package io.oris.sdk.store;

import com.fasterxml.jackson.databind.ObjectMapper;

import java.sql.*;
import java.time.Instant;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

public class SqliteGeneStore implements GeneStore {

    private static final ObjectMapper MAPPER = new ObjectMapper();
    private final Connection conn;

    public SqliteGeneStore(String path) {
        try {
            this.conn = DriverManager.getConnection("jdbc:sqlite:" + path);
            conn.createStatement().execute("PRAGMA journal_mode = WAL");
            conn.createStatement().execute("PRAGMA busy_timeout = 5000");
            migrate();
        } catch (SQLException e) {
            throw new RuntimeException("failed to open sqlite store", e);
        }
    }

    private void migrate() throws SQLException {
        conn.createStatement().executeUpdate(
            "CREATE TABLE IF NOT EXISTS genes (" +
            "gene_id TEXT PRIMARY KEY, name TEXT NOT NULL, task_class TEXT NOT NULL, " +
            "confidence REAL NOT NULL DEFAULT 0.0, strategy TEXT, signals TEXT, validation TEXT, " +
            "quality_score REAL DEFAULT 0.0, use_count INTEGER DEFAULT 0, success_count INTEGER DEFAULT 0, " +
            "contributor_id TEXT DEFAULT '', source TEXT NOT NULL DEFAULT 'local', " +
            "synced_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)"
        );
        conn.createStatement().executeUpdate("CREATE INDEX IF NOT EXISTS idx_genes_task_class ON genes(task_class)");
        conn.createStatement().executeUpdate("CREATE INDEX IF NOT EXISTS idx_genes_confidence ON genes(confidence DESC)");
        conn.createStatement().executeUpdate(
            "CREATE TABLE IF NOT EXISTS sync_log (" +
            "id INTEGER PRIMARY KEY AUTOINCREMENT, direction TEXT NOT NULL, gene_id TEXT NOT NULL, " +
            "status TEXT NOT NULL, remote_url TEXT, error_message TEXT, timestamp TEXT NOT NULL)"
        );
    }

    @Override
    public void save(Gene gene) {
        String sql = "INSERT OR REPLACE INTO genes (gene_id, name, task_class, confidence, strategy, signals, " +
                "validation, quality_score, use_count, success_count, contributor_id, source, synced_at, created_at, updated_at) " +
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
        try (PreparedStatement ps = conn.prepareStatement(sql)) {
            ps.setString(1, gene.getGeneId());
            ps.setString(2, gene.getName());
            ps.setString(3, gene.getTaskClass());
            ps.setDouble(4, gene.getConfidence());
            ps.setString(5, toJson(gene.getStrategy()));
            ps.setString(6, toJson(gene.getSignals()));
            ps.setString(7, toJson(gene.getValidation()));
            ps.setDouble(8, gene.getQualityScore());
            ps.setInt(9, gene.getUseCount());
            ps.setInt(10, gene.getSuccessCount());
            ps.setString(11, gene.getContributorId());
            ps.setString(12, gene.getSource() != null ? gene.getSource() : "local");
            ps.setString(13, gene.getSyncedAt() != null ? gene.getSyncedAt().toString() : null);
            ps.setString(14, gene.getCreatedAt() != null ? gene.getCreatedAt().toString() : Instant.now().toString());
            ps.setString(15, gene.getUpdatedAt() != null ? gene.getUpdatedAt().toString() : Instant.now().toString());
            ps.executeUpdate();
        } catch (SQLException e) {
            throw new RuntimeException("save gene failed", e);
        }
    }

    @Override
    public Gene get(String geneId) {
        try (PreparedStatement ps = conn.prepareStatement("SELECT * FROM genes WHERE gene_id = ?")) {
            ps.setString(1, geneId);
            ResultSet rs = ps.executeQuery();
            if (rs.next()) return mapRow(rs);
            return null;
        } catch (SQLException e) {
            throw new RuntimeException("get gene failed", e);
        }
    }

    @Override
    public void delete(String geneId) {
        try (PreparedStatement ps = conn.prepareStatement("DELETE FROM genes WHERE gene_id = ?")) {
            ps.setString(1, geneId);
            ps.executeUpdate();
        } catch (SQLException e) {
            throw new RuntimeException("delete gene failed", e);
        }
    }

    @Override
    public List<Gene> query(StoreQuery query) {
        StringBuilder sql = new StringBuilder("SELECT * FROM genes WHERE 1=1");
        List<Object> params = new ArrayList<>();
        if (query.getTaskClass() != null && !query.getTaskClass().isEmpty()) {
            sql.append(" AND task_class = ?");
            params.add(query.getTaskClass());
        }
        if (query.getMinConfidence() > 0) {
            sql.append(" AND confidence >= ?");
            params.add(query.getMinConfidence());
        }
        if (query.getSource() != null && !query.getSource().isEmpty()) {
            sql.append(" AND source = ?");
            params.add(query.getSource());
        }
        sql.append(" ORDER BY confidence DESC");
        int limit = query.getLimit() > 0 ? query.getLimit() : 50;
        sql.append(" LIMIT ?");
        params.add(limit);
        if (query.getOffset() > 0) {
            sql.append(" OFFSET ?");
            params.add(query.getOffset());
        }

        try (PreparedStatement ps = conn.prepareStatement(sql.toString())) {
            for (int i = 0; i < params.size(); i++) {
                Object p = params.get(i);
                if (p instanceof String) ps.setString(i + 1, (String) p);
                else if (p instanceof Double) ps.setDouble(i + 1, (Double) p);
                else if (p instanceof Integer) ps.setInt(i + 1, (Integer) p);
            }
            ResultSet rs = ps.executeQuery();
            List<Gene> results = new ArrayList<>();
            while (rs.next()) results.add(mapRow(rs));
            return results;
        } catch (SQLException e) {
            throw new RuntimeException("query genes failed", e);
        }
    }

    @Override
    public void updateStats(String geneId, boolean used, boolean success) {
        String sql = "UPDATE genes SET use_count = use_count + ?, success_count = success_count + ?, updated_at = ? WHERE gene_id = ?";
        try (PreparedStatement ps = conn.prepareStatement(sql)) {
            ps.setInt(1, used ? 1 : 0);
            ps.setInt(2, success ? 1 : 0);
            ps.setString(3, Instant.now().toString());
            ps.setString(4, geneId);
            ps.executeUpdate();
        } catch (SQLException e) {
            throw new RuntimeException("updateStats failed", e);
        }
    }

    @Override
    public List<Gene> list(int limit, int offset) {
        StoreQuery q = new StoreQuery();
        q.setLimit(limit);
        q.setOffset(offset);
        return query(q);
    }

    @Override
    public List<Gene> getUnsynced() {
        try (PreparedStatement ps = conn.prepareStatement("SELECT * FROM genes WHERE synced_at IS NULL AND source = 'local'")) {
            ResultSet rs = ps.executeQuery();
            List<Gene> results = new ArrayList<>();
            while (rs.next()) results.add(mapRow(rs));
            return results;
        } catch (SQLException e) {
            throw new RuntimeException("getUnsynced failed", e);
        }
    }

    @Override
    public void markSynced(String geneId, Instant syncedAt) {
        try (PreparedStatement ps = conn.prepareStatement("UPDATE genes SET synced_at = ? WHERE gene_id = ?")) {
            ps.setString(1, syncedAt.toString());
            ps.setString(2, geneId);
            ps.executeUpdate();
        } catch (SQLException e) {
            throw new RuntimeException("markSynced failed", e);
        }
    }

    @Override
    public void logSync(SyncLogEntry entry) {
        String sql = "INSERT INTO sync_log (direction, gene_id, status, remote_url, error_message, timestamp) VALUES (?, ?, ?, ?, ?, ?)";
        try (PreparedStatement ps = conn.prepareStatement(sql)) {
            ps.setString(1, entry.getDirection());
            ps.setString(2, entry.getGeneId());
            ps.setString(3, entry.getStatus());
            ps.setString(4, entry.getRemoteUrl());
            ps.setString(5, entry.getErrorMessage());
            ps.setString(6, entry.getTimestamp() != null ? entry.getTimestamp().toString() : Instant.now().toString());
            ps.executeUpdate();
        } catch (SQLException e) {
            throw new RuntimeException("logSync failed", e);
        }
    }

    @Override
    public List<SyncLogEntry> getSyncLog(int limit) {
        try (PreparedStatement ps = conn.prepareStatement("SELECT * FROM sync_log ORDER BY id DESC LIMIT ?")) {
            ps.setInt(1, limit);
            ResultSet rs = ps.executeQuery();
            List<SyncLogEntry> results = new ArrayList<>();
            while (rs.next()) {
                SyncLogEntry e = new SyncLogEntry();
                e.setId(rs.getInt("id"));
                e.setDirection(rs.getString("direction"));
                e.setGeneId(rs.getString("gene_id"));
                e.setStatus(rs.getString("status"));
                e.setRemoteUrl(rs.getString("remote_url"));
                e.setErrorMessage(rs.getString("error_message"));
                String ts = rs.getString("timestamp");
                if (ts != null) e.setTimestamp(Instant.parse(ts));
                results.add(e);
            }
            return results;
        } catch (SQLException e) {
            throw new RuntimeException("getSyncLog failed", e);
        }
    }

    @Override
    public void close() throws Exception {
        conn.close();
    }

    @SuppressWarnings("unchecked")
    private Gene mapRow(ResultSet rs) throws SQLException {
        Gene g = new Gene();
        g.setGeneId(rs.getString("gene_id"));
        g.setName(rs.getString("name"));
        g.setTaskClass(rs.getString("task_class"));
        g.setConfidence(rs.getDouble("confidence"));
        g.setStrategy(fromJson(rs.getString("strategy")));
        g.setSignals(fromJson(rs.getString("signals")));
        g.setValidation(fromJson(rs.getString("validation")));
        g.setQualityScore(rs.getDouble("quality_score"));
        g.setUseCount(rs.getInt("use_count"));
        g.setSuccessCount(rs.getInt("success_count"));
        g.setContributorId(rs.getString("contributor_id"));
        g.setSource(rs.getString("source"));
        String synced = rs.getString("synced_at");
        if (synced != null) g.setSyncedAt(Instant.parse(synced));
        String created = rs.getString("created_at");
        if (created != null) g.setCreatedAt(Instant.parse(created));
        String updated = rs.getString("updated_at");
        if (updated != null) g.setUpdatedAt(Instant.parse(updated));
        return g;
    }

    private String toJson(Object obj) {
        if (obj == null) return null;
        try { return MAPPER.writeValueAsString(obj); } catch (Exception e) { return null; }
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> fromJson(String json) {
        if (json == null || json.isEmpty()) return null;
        try { return MAPPER.readValue(json, Map.class); } catch (Exception e) { return null; }
    }
}
