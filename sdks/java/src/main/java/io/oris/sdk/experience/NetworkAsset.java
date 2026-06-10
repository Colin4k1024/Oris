package io.oris.sdk.experience;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonIgnoreProperties(ignoreUnknown = true)
public class NetworkAsset {
    @JsonProperty("id") private String id;
    @JsonProperty("type") private String type;
    @JsonProperty("confidence") private double confidence;
    @JsonProperty("quality_score") private double qualityScore;
    @JsonProperty("use_count") private int useCount;
    @JsonProperty("success_count") private int successCount;
    @JsonProperty("contributor_id") private String contributorId;
    @JsonProperty("created_at") private String createdAt;

    public String getId() { return id; }
    public void setId(String id) { this.id = id; }
    public String getType() { return type; }
    public void setType(String type) { this.type = type; }
    public double getConfidence() { return confidence; }
    public void setConfidence(double confidence) { this.confidence = confidence; }
    public double getQualityScore() { return qualityScore; }
    public void setQualityScore(double qualityScore) { this.qualityScore = qualityScore; }
    public int getUseCount() { return useCount; }
    public void setUseCount(int useCount) { this.useCount = useCount; }
    public int getSuccessCount() { return successCount; }
    public void setSuccessCount(int successCount) { this.successCount = successCount; }
    public String getContributorId() { return contributorId; }
    public void setContributorId(String contributorId) { this.contributorId = contributorId; }
    public String getCreatedAt() { return createdAt; }
    public void setCreatedAt(String createdAt) { this.createdAt = createdAt; }
}
