package io.oris.sdk.sync;

import io.oris.sdk.experience.ExperienceClient;
import io.oris.sdk.experience.FetchQuery;
import io.oris.sdk.experience.FetchResponse;
import io.oris.sdk.experience.NetworkAsset;
import io.oris.sdk.hub.HubClient;
import io.oris.sdk.hub.RegisterResponse;
import io.oris.sdk.store.Gene;
import io.oris.sdk.store.GeneStore;
import io.oris.sdk.store.SyncLogEntry;

import java.io.IOException;
import java.time.Instant;
import java.util.*;

public class SyncManager {

    private final GeneStore store;
    private final ExperienceClient experience;
    private final HubClient hub;

    public SyncManager(GeneStore store, ExperienceClient experience, HubClient hub) {
        this.store = store;
        this.experience = experience;
        this.hub = hub;
    }

    public PushResult pushToHub(PushOpts opts) throws IOException {
        if (experience == null) throw new IllegalStateException("experience client not configured");

        List<Gene> genes;
        if (opts != null && opts.getGeneIds() != null && !opts.getGeneIds().isEmpty()) {
            genes = new ArrayList<>();
            for (String id : opts.getGeneIds()) {
                Gene g = store.get(id);
                if (g != null) genes.add(g);
            }
        } else {
            genes = store.getUnsynced();
        }

        PushResult result = new PushResult();
        for (Gene gene : genes) {
            Map<String, Object> payload = geneToPayload(gene);
            SyncLogEntry entry = new SyncLogEntry("push", gene.getGeneId(), "", Instant.now());

            try {
                experience.share(payload);
                entry.setStatus("success");
                result.setPushed(result.getPushed() + 1);
                store.markSynced(gene.getGeneId(), Instant.now());
            } catch (Exception e) {
                entry.setStatus("failed");
                entry.setErrorMessage(e.getMessage());
                result.setFailed(result.getFailed() + 1);
                result.getErrors().add(new PushError(gene.getGeneId(), e.getMessage()));
            }
            store.logSync(entry);
        }
        return result;
    }

    public int pullFromHub(PullOpts opts) throws IOException {
        if (experience == null) throw new IllegalStateException("experience client not configured");

        FetchQuery query = new FetchQuery();
        if (opts != null) {
            query.setQ(opts.getQ());
            query.setMinConfidence(opts.getMinConfidence());
            query.setLimit(opts.getLimit());
        }

        FetchResponse resp = experience.fetch(query);
        int imported = 0;

        for (NetworkAsset asset : resp.getAssets()) {
            Gene gene = assetToGene(asset);
            Gene existing = store.get(gene.getGeneId());

            if (existing != null) {
                if (coreFieldsMatch(existing, gene)) {
                    gene = mergeStats(existing, gene);
                } else {
                    SyncLogEntry entry = new SyncLogEntry("pull", gene.getGeneId(), "conflict", Instant.now());
                    entry.setErrorMessage("core fields differ, skipped");
                    store.logSync(entry);
                    continue;
                }
            }

            store.save(gene);
            store.logSync(new SyncLogEntry("pull", gene.getGeneId(), "success", Instant.now()));
            imported++;
        }
        return imported;
    }

    public RegisterResponse registerNode(String endpoint, List<String> capabilities, String version, String region) throws IOException {
        if (hub == null) throw new IllegalStateException("hub client not configured");
        return hub.register(endpoint, capabilities, version, region);
    }

    public List<SyncLogEntry> getSyncLog(int limit) {
        return store.getSyncLog(limit);
    }

    private Map<String, Object> geneToPayload(Gene g) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("type", "gene");
        p.put("id", g.getGeneId());
        p.put("confidence", g.getConfidence());
        p.put("quality_score", g.getQualityScore());
        p.put("use_count", g.getUseCount());
        p.put("success_count", g.getSuccessCount());
        p.put("created_at", g.getCreatedAt() != null ? g.getCreatedAt().toString() : Instant.now().toString());
        if (g.getStrategy() != null) p.put("strategy", g.getStrategy());
        if (g.getSignals() != null) p.put("signals", g.getSignals());
        if (g.getValidation() != null) p.put("validation", g.getValidation());
        if (g.getContributorId() != null && !g.getContributorId().isEmpty()) p.put("contributor_id", g.getContributorId());
        return p;
    }

    private Gene assetToGene(NetworkAsset a) {
        Gene g = new Gene();
        g.setGeneId(a.getId());
        g.setName(a.getId());
        g.setTaskClass("unknown");
        g.setConfidence(a.getConfidence());
        g.setQualityScore(a.getQualityScore());
        g.setUseCount(a.getUseCount());
        g.setSuccessCount(a.getSuccessCount());
        g.setContributorId(a.getContributorId() != null ? a.getContributorId() : "");
        g.setSource("hub");
        Instant now = Instant.now();
        g.setSyncedAt(now);
        g.setCreatedAt(now);
        g.setUpdatedAt(now);
        return g;
    }

    private boolean coreFieldsMatch(Gene existing, Gene incoming) {
        return Objects.equals(existing.getTaskClass(), incoming.getTaskClass());
    }

    private Gene mergeStats(Gene existing, Gene incoming) {
        existing.setUseCount(Math.max(existing.getUseCount(), incoming.getUseCount()));
        existing.setSuccessCount(Math.max(existing.getSuccessCount(), incoming.getSuccessCount()));
        existing.setConfidence(Math.max(existing.getConfidence(), incoming.getConfidence()));
        existing.setQualityScore(Math.max(existing.getQualityScore(), incoming.getQualityScore()));
        existing.setUpdatedAt(Instant.now());
        existing.setSyncedAt(Instant.now());
        return existing;
    }
}
