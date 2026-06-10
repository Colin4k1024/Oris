package io.oris.sdk.sync;

public class PushError {
    private final String geneId;
    private final String message;

    public PushError(String geneId, String message) {
        this.geneId = geneId;
        this.message = message;
    }

    public String getGeneId() { return geneId; }
    public String getMessage() { return message; }
}
