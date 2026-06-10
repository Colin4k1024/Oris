package io.oris.sdk.hub;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonIgnoreProperties(ignoreUnknown = true)
public class HeartbeatResponse {
    @JsonProperty("acknowledged") private boolean acknowledged;
    @JsonProperty("next_heartbeat_seconds") private int nextHeartbeatSeconds;

    public boolean isAcknowledged() { return acknowledged; }
    public void setAcknowledged(boolean acknowledged) { this.acknowledged = acknowledged; }
    public int getNextHeartbeatSeconds() { return nextHeartbeatSeconds; }
    public void setNextHeartbeatSeconds(int v) { this.nextHeartbeatSeconds = v; }
}
