package io.oris.sdk.hub;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.List;

@JsonInclude(JsonInclude.Include.NON_NULL)
public class FederatedQuery {
    @JsonProperty("query") private String query;
    @JsonProperty("task_class") private String taskClass;
    @JsonProperty("min_confidence") private Double minConfidence;
    @JsonProperty("timeout_ms") private Long timeoutMs;
    @JsonProperty("target_nodes") private List<String> targetNodes;
    @JsonProperty("limit") private Integer limit;

    public FederatedQuery() {}

    public FederatedQuery(String query) { this.query = query; }

    public String getQuery() { return query; }
    public void setQuery(String query) { this.query = query; }
    public String getTaskClass() { return taskClass; }
    public void setTaskClass(String taskClass) { this.taskClass = taskClass; }
    public Double getMinConfidence() { return minConfidence; }
    public void setMinConfidence(Double minConfidence) { this.minConfidence = minConfidence; }
    public Long getTimeoutMs() { return timeoutMs; }
    public void setTimeoutMs(Long timeoutMs) { this.timeoutMs = timeoutMs; }
    public List<String> getTargetNodes() { return targetNodes; }
    public void setTargetNodes(List<String> targetNodes) { this.targetNodes = targetNodes; }
    public Integer getLimit() { return limit; }
    public void setLimit(Integer limit) { this.limit = limit; }
}
