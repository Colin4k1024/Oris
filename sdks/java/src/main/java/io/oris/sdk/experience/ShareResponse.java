package io.oris.sdk.experience;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonIgnoreProperties(ignoreUnknown = true)
public class ShareResponse {
    @JsonProperty("gene_id") private String geneId;
    @JsonProperty("status") private String status;
    @JsonProperty("published_at") private String publishedAt;

    public String getGeneId() { return geneId; }
    public void setGeneId(String geneId) { this.geneId = geneId; }
    public String getStatus() { return status; }
    public void setStatus(String status) { this.status = status; }
    public String getPublishedAt() { return publishedAt; }
    public void setPublishedAt(String publishedAt) { this.publishedAt = publishedAt; }
}
