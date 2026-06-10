package io.oris.sdk;

import io.oris.sdk.store.*;
import org.junit.jupiter.api.*;

import java.time.Instant;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class StoreTest {

    private SqliteGeneStore store;

    @BeforeEach
    void setUp() {
        store = new SqliteGeneStore(":memory:");
    }

    @AfterEach
    void tearDown() throws Exception {
        store.close();
    }

    private Gene sampleGene(String id) {
        Gene g = new Gene();
        g.setGeneId(id);
        g.setName("test-gene-" + id);
        g.setTaskClass("error-handling");
        g.setConfidence(0.85);
        g.setQualityScore(0.9);
        g.setUseCount(5);
        g.setSuccessCount(4);
        g.setSource("local");
        g.setCreatedAt(Instant.now());
        g.setUpdatedAt(Instant.now());
        return g;
    }

    @Test
    void saveAndGet() {
        Gene gene = sampleGene("g1");
        store.save(gene);
        Gene loaded = store.get("g1");
        assertNotNull(loaded);
        assertEquals("g1", loaded.getGeneId());
        assertEquals(0.85, loaded.getConfidence(), 0.001);
    }

    @Test
    void getReturnsNullForMissing() {
        assertNull(store.get("nonexistent"));
    }

    @Test
    void deleteRemovesGene() {
        store.save(sampleGene("g2"));
        store.delete("g2");
        assertNull(store.get("g2"));
    }

    @Test
    void queryByTaskClass() {
        store.save(sampleGene("g1"));
        Gene other = sampleGene("g2");
        other.setTaskClass("other-class");
        store.save(other);

        StoreQuery q = new StoreQuery();
        q.setTaskClass("error-handling");
        List<Gene> results = store.query(q);
        assertEquals(1, results.size());
        assertEquals("g1", results.get(0).getGeneId());
    }

    @Test
    void queryByMinConfidence() {
        store.save(sampleGene("g1"));
        Gene low = sampleGene("g2");
        low.setConfidence(0.3);
        store.save(low);

        StoreQuery q = new StoreQuery();
        q.setMinConfidence(0.5);
        List<Gene> results = store.query(q);
        assertEquals(1, results.size());
    }

    @Test
    void updateStatsIncrementsCounters() {
        store.save(sampleGene("g1"));
        store.updateStats("g1", true, true);
        Gene loaded = store.get("g1");
        assertEquals(6, loaded.getUseCount());
        assertEquals(5, loaded.getSuccessCount());
    }

    @Test
    void unsyncedReturnsLocalWithNoSyncedAt() {
        Gene gene = sampleGene("g1");
        store.save(gene);
        List<Gene> unsynced = store.getUnsynced();
        assertEquals(1, unsynced.size());

        store.markSynced("g1", Instant.now());
        unsynced = store.getUnsynced();
        assertTrue(unsynced.isEmpty());
    }

    @Test
    void syncLogRecordsEntries() {
        SyncLogEntry entry = new SyncLogEntry("push", "g1", "success", Instant.now());
        store.logSync(entry);
        List<SyncLogEntry> log = store.getSyncLog(10);
        assertEquals(1, log.size());
        assertEquals("push", log.get(0).getDirection());
        assertEquals("g1", log.get(0).getGeneId());
    }
}
