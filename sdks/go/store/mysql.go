package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	_ "github.com/go-sql-driver/mysql"
)

type MySQLConfig struct {
	Host     string
	Port     int
	User     string
	Password string
	Database string
	Params   map[string]string
}

func (c MySQLConfig) DSN() string {
	host := c.Host
	if host == "" {
		host = "127.0.0.1"
	}
	port := c.Port
	if port == 0 {
		port = 3306
	}
	db := c.Database
	if db == "" {
		db = "oris"
	}
	var b strings.Builder
	fmt.Fprintf(&b, "%s:%s@tcp(%s:%d)/%s?parseTime=true&charset=utf8mb4",
		c.User, c.Password, host, port, db)
	for k, v := range c.Params {
		b.WriteString("&")
		b.WriteString(k)
		b.WriteString("=")
		b.WriteString(v)
	}
	return b.String()
}

type MySQLStore struct {
	db *sql.DB
}

func OpenMySQL(cfg MySQLConfig) (*MySQLStore, error) {
	db, err := sql.Open("mysql", cfg.DSN())
	if err != nil {
		return nil, fmt.Errorf("open mysql: %w", err)
	}
	if err := db.Ping(); err != nil {
		db.Close()
		return nil, fmt.Errorf("ping mysql: %w", err)
	}

	db.SetMaxOpenConns(25)
	db.SetMaxIdleConns(5)
	db.SetConnMaxLifetime(5 * time.Minute)

	if err := migrateMySQL(db); err != nil {
		db.Close()
		return nil, fmt.Errorf("migrate mysql: %w", err)
	}
	return &MySQLStore{db: db}, nil
}

func OpenMySQLFromDSN(dsn string) (*MySQLStore, error) {
	db, err := sql.Open("mysql", dsn)
	if err != nil {
		return nil, fmt.Errorf("open mysql: %w", err)
	}
	if err := db.Ping(); err != nil {
		db.Close()
		return nil, fmt.Errorf("ping mysql: %w", err)
	}

	db.SetMaxOpenConns(25)
	db.SetMaxIdleConns(5)
	db.SetConnMaxLifetime(5 * time.Minute)

	if err := migrateMySQL(db); err != nil {
		db.Close()
		return nil, fmt.Errorf("migrate mysql: %w", err)
	}
	return &MySQLStore{db: db}, nil
}

func migrateMySQL(db *sql.DB) error {
	stmts := []string{
		`CREATE TABLE IF NOT EXISTS genes (
			gene_id        VARCHAR(255) PRIMARY KEY,
			name           VARCHAR(512) NOT NULL,
			task_class     VARCHAR(255) NOT NULL,
			confidence     DOUBLE NOT NULL DEFAULT 0.0,
			strategy       JSON,
			signals        JSON,
			validation     JSON,
			quality_score  DOUBLE DEFAULT 0.0,
			use_count      INT DEFAULT 0,
			success_count  INT DEFAULT 0,
			contributor_id VARCHAR(255) DEFAULT '',
			source         VARCHAR(64) NOT NULL DEFAULT 'local',
			synced_at      DATETIME(3),
			created_at     DATETIME(3) NOT NULL,
			updated_at     DATETIME(3) NOT NULL,
			INDEX idx_genes_task_class (task_class),
			INDEX idx_genes_confidence (confidence DESC),
			INDEX idx_genes_source (source),
			INDEX idx_genes_synced_at (synced_at)
		) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4`,
		`CREATE TABLE IF NOT EXISTS sync_log (
			id             BIGINT AUTO_INCREMENT PRIMARY KEY,
			direction      VARCHAR(16) NOT NULL,
			gene_id        VARCHAR(255) NOT NULL,
			status         VARCHAR(32) NOT NULL,
			remote_url     TEXT,
			error_message  TEXT,
			timestamp      DATETIME(3) NOT NULL,
			INDEX idx_sync_log_timestamp (timestamp DESC),
			INDEX idx_sync_log_gene (gene_id)
		) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4`,
	}
	for _, stmt := range stmts {
		if _, err := db.Exec(stmt); err != nil {
			if !strings.Contains(err.Error(), "Duplicate") {
				return err
			}
		}
	}
	return nil
}

func (s *MySQLStore) Close() error {
	return s.db.Close()
}

func (s *MySQLStore) Save(ctx context.Context, gene Gene) error {
	strategyJSON, _ := json.Marshal(gene.Strategy)
	signalsJSON, _ := json.Marshal(gene.Signals)
	validationJSON, _ := json.Marshal(gene.Validation)

	_, err := s.db.ExecContext(ctx, `
		INSERT INTO genes
		(gene_id, name, task_class, confidence, strategy, signals, validation,
		 quality_score, use_count, success_count, contributor_id, source, synced_at, created_at, updated_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON DUPLICATE KEY UPDATE
			name=VALUES(name), task_class=VALUES(task_class), confidence=VALUES(confidence),
			strategy=VALUES(strategy), signals=VALUES(signals), validation=VALUES(validation),
			quality_score=VALUES(quality_score), use_count=VALUES(use_count),
			success_count=VALUES(success_count), contributor_id=VALUES(contributor_id),
			source=VALUES(source), synced_at=VALUES(synced_at), updated_at=VALUES(updated_at)`,
		gene.GeneID, gene.Name, gene.TaskClass, gene.Confidence,
		string(strategyJSON), string(signalsJSON), string(validationJSON),
		gene.QualityScore, gene.UseCount, gene.SuccessCount,
		gene.ContributorID, gene.Source, gene.SyncedAt,
		gene.CreatedAt, gene.UpdatedAt,
	)
	return err
}

func (s *MySQLStore) Get(ctx context.Context, geneID string) (*Gene, error) {
	row := s.db.QueryRowContext(ctx, `SELECT gene_id, name, task_class, confidence,
		strategy, signals, validation, quality_score, use_count, success_count,
		contributor_id, source, synced_at, created_at, updated_at
		FROM genes WHERE gene_id = ?`, geneID)
	return scanGeneMySQL(row)
}

func (s *MySQLStore) Query(ctx context.Context, q StoreQuery) ([]Gene, error) {
	where, args := buildWhere(q)
	orderBy := "created_at DESC"
	if q.OrderBy != "" {
		orderBy = q.OrderBy
	}
	limit := 50
	if q.Limit > 0 {
		limit = q.Limit
	}

	query := fmt.Sprintf(`SELECT gene_id, name, task_class, confidence,
		strategy, signals, validation, quality_score, use_count, success_count,
		contributor_id, source, synced_at, created_at, updated_at
		FROM genes %s ORDER BY %s LIMIT ? OFFSET ?`, where, orderBy)

	args = append(args, limit, q.Offset)
	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var genes []Gene
	for rows.Next() {
		g, err := scanGeneMySQLRows(rows)
		if err != nil {
			return nil, err
		}
		genes = append(genes, *g)
	}
	return genes, rows.Err()
}

func (s *MySQLStore) Delete(ctx context.Context, geneID string) error {
	_, err := s.db.ExecContext(ctx, "DELETE FROM genes WHERE gene_id = ?", geneID)
	return err
}

func (s *MySQLStore) UpdateStats(ctx context.Context, geneID string, used bool, success bool) error {
	now := time.Now().UTC()
	if used && success {
		_, err := s.db.ExecContext(ctx,
			"UPDATE genes SET use_count = use_count + 1, success_count = success_count + 1, updated_at = ? WHERE gene_id = ?",
			now, geneID)
		return err
	}
	if used {
		_, err := s.db.ExecContext(ctx,
			"UPDATE genes SET use_count = use_count + 1, updated_at = ? WHERE gene_id = ?",
			now, geneID)
		return err
	}
	return nil
}

func (s *MySQLStore) List(ctx context.Context, opts ListOpts) ([]Gene, error) {
	limit := 50
	if opts.Limit > 0 {
		limit = opts.Limit
	}
	return s.Query(ctx, StoreQuery{Limit: limit, Offset: opts.Offset})
}

func (s *MySQLStore) GetUnsynced(ctx context.Context) ([]Gene, error) {
	return s.Query(ctx, StoreQuery{Source: "local", Limit: 1000})
}

func (s *MySQLStore) MarkSynced(ctx context.Context, geneID string, syncedAt time.Time) error {
	_, err := s.db.ExecContext(ctx,
		"UPDATE genes SET synced_at = ?, updated_at = ? WHERE gene_id = ?",
		syncedAt, time.Now().UTC(), geneID)
	return err
}

func (s *MySQLStore) LogSync(ctx context.Context, entry SyncLogEntry) error {
	_, err := s.db.ExecContext(ctx,
		"INSERT INTO sync_log (direction, gene_id, status, remote_url, error_message, timestamp) VALUES (?, ?, ?, ?, ?, ?)",
		entry.Direction, entry.GeneID, entry.Status, entry.RemoteURL, entry.ErrorMessage, entry.Timestamp)
	return err
}

func (s *MySQLStore) GetSyncLog(ctx context.Context, limit int) ([]SyncLogEntry, error) {
	if limit <= 0 {
		limit = 50
	}
	rows, err := s.db.QueryContext(ctx,
		"SELECT id, direction, gene_id, status, remote_url, error_message, timestamp FROM sync_log ORDER BY timestamp DESC LIMIT ?", limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var entries []SyncLogEntry
	for rows.Next() {
		var e SyncLogEntry
		if err := rows.Scan(&e.ID, &e.Direction, &e.GeneID, &e.Status, &e.RemoteURL, &e.ErrorMessage, &e.Timestamp); err != nil {
			return nil, err
		}
		entries = append(entries, e)
	}
	return entries, rows.Err()
}

func scanGeneMySQL(row scanner) (*Gene, error) {
	var g Gene
	var strategyJSON, signalsJSON, validationJSON sql.NullString
	var syncedAt sql.NullTime
	var createdAt, updatedAt time.Time

	err := row.Scan(&g.GeneID, &g.Name, &g.TaskClass, &g.Confidence,
		&strategyJSON, &signalsJSON, &validationJSON,
		&g.QualityScore, &g.UseCount, &g.SuccessCount,
		&g.ContributorID, &g.Source, &syncedAt, &createdAt, &updatedAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}

	if strategyJSON.Valid {
		json.Unmarshal([]byte(strategyJSON.String), &g.Strategy)
	}
	if signalsJSON.Valid {
		json.Unmarshal([]byte(signalsJSON.String), &g.Signals)
	}
	if validationJSON.Valid {
		json.Unmarshal([]byte(validationJSON.String), &g.Validation)
	}
	if syncedAt.Valid {
		g.SyncedAt = &syncedAt.Time
	}
	g.CreatedAt = createdAt
	g.UpdatedAt = updatedAt

	return &g, nil
}

func scanGeneMySQLRows(rows *sql.Rows) (*Gene, error) {
	return scanGeneMySQL(rows)
}
