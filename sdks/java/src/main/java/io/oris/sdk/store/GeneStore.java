package io.oris.sdk.store;

import java.time.Instant;
import java.util.List;

public interface GeneStore extends AutoCloseable {
    void save(Gene gene);
    Gene get(String geneId);
    void delete(String geneId);
    List<Gene> query(StoreQuery query);
    void updateStats(String geneId, boolean used, boolean success);
    List<Gene> list(int limit, int offset);
    List<Gene> getUnsynced();
    void markSynced(String geneId, Instant syncedAt);
    void logSync(SyncLogEntry entry);
    List<SyncLogEntry> getSyncLog(int limit);
}
