package io.oris.sdk.hub;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonIgnoreProperties(ignoreUnknown = true)
public class RegisterResponse {
    @JsonProperty("node_id") private String nodeId;
    @JsonProperty("heartbeat_interval_seconds") private int heartbeatIntervalSeconds;
    @JsonProperty("ttl_seconds") private int ttlSeconds;

    public String getNodeId() { return nodeId; }
    public void setNodeId(String nodeId) { this.nodeId = nodeId; }
    public int getHeartbeatIntervalSeconds() { return heartbeatIntervalSeconds; }
    public void setHeartbeatIntervalSeconds(int v) { this.heartbeatIntervalSeconds = v; }
    public int getTtlSeconds() { return ttlSeconds; }
    public void setTtlSeconds(int ttlSeconds) { this.ttlSeconds = ttlSeconds; }
}
