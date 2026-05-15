package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"time"

	_ "modernc.org/sqlite"
)

type LocalStore struct {
	db *sql.DB
	mu sync.RWMutex
}

func Open(path string) (*LocalStore, error) {
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}

	if _, err := db.Exec("PRAGMA journal_mode=WAL"); err != nil {
		db.Close()
		return nil, fmt.Errorf("set WAL mode: %w", err)
	}
	if _, err := db.Exec("PRAGMA busy_timeout=5000"); err != nil {
		db.Close()
		return nil, fmt.Errorf("set busy_timeout: %w", err)
	}

	if err := migrate(db); err != nil {
		db.Close()
		return nil, fmt.Errorf("migrate: %w", err)
	}

	return &LocalStore{db: db}, nil
}

func (s *LocalStore) Close() error {
	return s.db.Close()
}

func migrate(db *sql.DB) error {
	schema := `
	CREATE TABLE IF NOT EXISTS genes (
		gene_id        TEXT PRIMARY KEY,
		name           TEXT NOT NULL,
		task_class     TEXT NOT NULL,
		confidence     REAL NOT NULL DEFAULT 0.0,
		strategy       TEXT,
		signals        TEXT,
		validation     TEXT,
		quality_score  REAL DEFAULT 0.0,
		use_count      INTEGER DEFAULT 0,
		success_count  INTEGER DEFAULT 0,
		contributor_id TEXT DEFAULT '',
		source         TEXT NOT NULL DEFAULT 'local',
		synced_at      TEXT,
		created_at     TEXT NOT NULL,
		updated_at     TEXT NOT NULL
	);
	CREATE INDEX IF NOT EXISTS idx_genes_task_class ON genes(task_class);
	CREATE INDEX IF NOT EXISTS idx_genes_confidence ON genes(confidence DESC);
	CREATE INDEX IF NOT EXISTS idx_genes_source ON genes(source);
	CREATE INDEX IF NOT EXISTS idx_genes_synced_at ON genes(synced_at);

	CREATE TABLE IF NOT EXISTS sync_log (
		id             INTEGER PRIMARY KEY AUTOINCREMENT,
		direction      TEXT NOT NULL,
		gene_id        TEXT NOT NULL,
		status         TEXT NOT NULL,
		remote_url     TEXT,
		error_message  TEXT,
		timestamp      TEXT NOT NULL
	);
	CREATE INDEX IF NOT EXISTS idx_sync_log_timestamp ON sync_log(timestamp DESC);
	CREATE INDEX IF NOT EXISTS idx_sync_log_gene ON sync_log(gene_id);
	`
	_, err := db.Exec(schema)
	return err
}

func (s *LocalStore) Save(ctx context.Context, gene Gene) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	strategyJSON, _ := json.Marshal(gene.Strategy)
	signalsJSON, _ := json.Marshal(gene.Signals)
	validationJSON, _ := json.Marshal(gene.Validation)

	var syncedAt *string
	if gene.SyncedAt != nil {
		t := gene.SyncedAt.Format(time.RFC3339)
		syncedAt = &t
	}

	_, err := s.db.ExecContext(ctx, `
		INSERT OR REPLACE INTO genes
		(gene_id, name, task_class, confidence, strategy, signals, validation,
		 quality_score, use_count, success_count, contributor_id, source, synced_at, created_at, updated_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		gene.GeneID, gene.Name, gene.TaskClass, gene.Confidence,
		string(strategyJSON), string(signalsJSON), string(validationJSON),
		gene.QualityScore, gene.UseCount, gene.SuccessCount,
		gene.ContributorID, gene.Source, syncedAt,
		gene.CreatedAt.Format(time.RFC3339), gene.UpdatedAt.Format(time.RFC3339),
	)
	return err
}

func (s *LocalStore) Get(ctx context.Context, geneID string) (*Gene, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	row := s.db.QueryRowContext(ctx, `SELECT gene_id, name, task_class, confidence,
		strategy, signals, validation, quality_score, use_count, success_count,
		contributor_id, source, synced_at, created_at, updated_at
		FROM genes WHERE gene_id = ?`, geneID)

	return scanGene(row)
}

func (s *LocalStore) Query(ctx context.Context, q StoreQuery) ([]Gene, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

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
		g, err := scanGeneRows(rows)
		if err != nil {
			return nil, err
		}
		genes = append(genes, *g)
	}
	return genes, rows.Err()
}

func (s *LocalStore) Delete(ctx context.Context, geneID string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.ExecContext(ctx, "DELETE FROM genes WHERE gene_id = ?", geneID)
	return err
}

func (s *LocalStore) UpdateStats(ctx context.Context, geneID string, used bool, success bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if used && success {
		_, err := s.db.ExecContext(ctx,
			"UPDATE genes SET use_count = use_count + 1, success_count = success_count + 1, updated_at = ? WHERE gene_id = ?",
			time.Now().UTC().Format(time.RFC3339), geneID)
		return err
	}
	if used {
		_, err := s.db.ExecContext(ctx,
			"UPDATE genes SET use_count = use_count + 1, updated_at = ? WHERE gene_id = ?",
			time.Now().UTC().Format(time.RFC3339), geneID)
		return err
	}
	return nil
}

func (s *LocalStore) List(ctx context.Context, opts ListOpts) ([]Gene, error) {
	limit := 50
	if opts.Limit > 0 {
		limit = opts.Limit
	}
	return s.Query(ctx, StoreQuery{Limit: limit, Offset: opts.Offset})
}

func (s *LocalStore) GetUnsynced(ctx context.Context) ([]Gene, error) {
	return s.Query(ctx, StoreQuery{Source: "local", Limit: 1000})
}

func (s *LocalStore) MarkSynced(ctx context.Context, geneID string, syncedAt time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.ExecContext(ctx,
		"UPDATE genes SET synced_at = ?, updated_at = ? WHERE gene_id = ?",
		syncedAt.Format(time.RFC3339), time.Now().UTC().Format(time.RFC3339), geneID)
	return err
}

func (s *LocalStore) LogSync(ctx context.Context, entry SyncLogEntry) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.ExecContext(ctx,
		"INSERT INTO sync_log (direction, gene_id, status, remote_url, error_message, timestamp) VALUES (?, ?, ?, ?, ?, ?)",
		entry.Direction, entry.GeneID, entry.Status, entry.RemoteURL, entry.ErrorMessage,
		entry.Timestamp.Format(time.RFC3339))
	return err
}

func (s *LocalStore) GetSyncLog(ctx context.Context, limit int) ([]SyncLogEntry, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

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
		var ts string
		if err := rows.Scan(&e.ID, &e.Direction, &e.GeneID, &e.Status, &e.RemoteURL, &e.ErrorMessage, &ts); err != nil {
			return nil, err
		}
		e.Timestamp, _ = time.Parse(time.RFC3339, ts)
		entries = append(entries, e)
	}
	return entries, rows.Err()
}

func buildWhere(q StoreQuery) (string, []any) {
	var clauses []string
	var args []any

	if q.TaskClass != "" {
		clauses = append(clauses, "task_class = ?")
		args = append(args, q.TaskClass)
	}
	if q.MinConfidence > 0 {
		clauses = append(clauses, "confidence >= ?")
		args = append(args, q.MinConfidence)
	}
	if q.Source != "" {
		clauses = append(clauses, "source = ?")
		args = append(args, q.Source)
	}
	if q.Q != "" {
		clauses = append(clauses, "(name LIKE ? OR gene_id LIKE ?)")
		pattern := "%" + q.Q + "%"
		args = append(args, pattern, pattern)
	}

	if len(clauses) == 0 {
		return "", nil
	}
	return "WHERE " + strings.Join(clauses, " AND "), args
}

type scanner interface {
	Scan(dest ...any) error
}

func scanGene(row scanner) (*Gene, error) {
	var g Gene
	var strategyJSON, signalsJSON, validationJSON sql.NullString
	var syncedAt sql.NullString
	var createdAt, updatedAt string

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
		t, _ := time.Parse(time.RFC3339, syncedAt.String)
		g.SyncedAt = &t
	}
	g.CreatedAt, _ = time.Parse(time.RFC3339, createdAt)
	g.UpdatedAt, _ = time.Parse(time.RFC3339, updatedAt)

	return &g, nil
}

func scanGeneRows(rows *sql.Rows) (*Gene, error) {
	return scanGene(rows)
}
