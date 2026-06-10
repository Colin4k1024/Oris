package io.oris.sdk.store;

import java.time.Instant;

public class SyncLogEntry {
    private int id;
    private String direction;
    private String geneId;
    private String status;
    private String remoteUrl;
    private String errorMessage;
    private Instant timestamp;

    public SyncLogEntry() {}

    public SyncLogEntry(String direction, String geneId, String status, Instant timestamp) {
        this.direction = direction;
        this.geneId = geneId;
        this.status = status;
        this.timestamp = timestamp;
    }

    public int getId() { return id; }
    public void setId(int id) { this.id = id; }
    public String getDirection() { return direction; }
    public void setDirection(String direction) { this.direction = direction; }
    public String getGeneId() { return geneId; }
    public void setGeneId(String geneId) { this.geneId = geneId; }
    public String getStatus() { return status; }
    public void setStatus(String status) { this.status = status; }
    public String getRemoteUrl() { return remoteUrl; }
    public void setRemoteUrl(String remoteUrl) { this.remoteUrl = remoteUrl; }
    public String getErrorMessage() { return errorMessage; }
    public void setErrorMessage(String errorMessage) { this.errorMessage = errorMessage; }
    public Instant getTimestamp() { return timestamp; }
    public void setTimestamp(Instant timestamp) { this.timestamp = timestamp; }
}
