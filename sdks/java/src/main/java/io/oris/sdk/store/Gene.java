package io.oris.sdk.store;

import java.time.Instant;
import java.util.Map;

public class Gene {
    private String geneId;
    private String name;
    private String taskClass;
    private double confidence;
    private Map<String, Object> strategy;
    private Map<String, Object> signals;
    private Map<String, Object> validation;
    private double qualityScore;
    private int useCount;
    private int successCount;
    private String contributorId;
    private String source;
    private Instant syncedAt;
    private Instant createdAt;
    private Instant updatedAt;

    public Gene() {}

    public String getGeneId() { return geneId; }
    public void setGeneId(String geneId) { this.geneId = geneId; }
    public String getName() { return name; }
    public void setName(String name) { this.name = name; }
    public String getTaskClass() { return taskClass; }
    public void setTaskClass(String taskClass) { this.taskClass = taskClass; }
    public double getConfidence() { return confidence; }
    public void setConfidence(double confidence) { this.confidence = confidence; }
    public Map<String, Object> getStrategy() { return strategy; }
    public void setStrategy(Map<String, Object> strategy) { this.strategy = strategy; }
    public Map<String, Object> getSignals() { return signals; }
    public void setSignals(Map<String, Object> signals) { this.signals = signals; }
    public Map<String, Object> getValidation() { return validation; }
    public void setValidation(Map<String, Object> validation) { this.validation = validation; }
    public double getQualityScore() { return qualityScore; }
    public void setQualityScore(double qualityScore) { this.qualityScore = qualityScore; }
    public int getUseCount() { return useCount; }
    public void setUseCount(int useCount) { this.useCount = useCount; }
    public int getSuccessCount() { return successCount; }
    public void setSuccessCount(int successCount) { this.successCount = successCount; }
    public String getContributorId() { return contributorId; }
    public void setContributorId(String contributorId) { this.contributorId = contributorId; }
    public String getSource() { return source; }
    public void setSource(String source) { this.source = source; }
    public Instant getSyncedAt() { return syncedAt; }
    public void setSyncedAt(Instant syncedAt) { this.syncedAt = syncedAt; }
    public Instant getCreatedAt() { return createdAt; }
    public void setCreatedAt(Instant createdAt) { this.createdAt = createdAt; }
    public Instant getUpdatedAt() { return updatedAt; }
    public void setUpdatedAt(Instant updatedAt) { this.updatedAt = updatedAt; }
}
