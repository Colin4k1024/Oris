package io.oris.sdk.experience;

public class FetchQuery {
    private String q;
    private double minConfidence;
    private int limit;
    private String cursor;

    public FetchQuery() {}

    public FetchQuery(String q, double minConfidence, int limit) {
        this.q = q;
        this.minConfidence = minConfidence;
        this.limit = limit;
    }

    public String getQ() { return q; }
    public void setQ(String q) { this.q = q; }
    public double getMinConfidence() { return minConfidence; }
    public void setMinConfidence(double minConfidence) { this.minConfidence = minConfidence; }
    public int getLimit() { return limit; }
    public void setLimit(int limit) { this.limit = limit; }
    public String getCursor() { return cursor; }
    public void setCursor(String cursor) { this.cursor = cursor; }
}
