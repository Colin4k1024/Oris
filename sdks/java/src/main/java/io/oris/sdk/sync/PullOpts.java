package io.oris.sdk.sync;

public class PullOpts {
    private String q;
    private double minConfidence;
    private int limit;

    public PullOpts() {}

    public String getQ() { return q; }
    public void setQ(String q) { this.q = q; }
    public double getMinConfidence() { return minConfidence; }
    public void setMinConfidence(double minConfidence) { this.minConfidence = minConfidence; }
    public int getLimit() { return limit; }
    public void setLimit(int limit) { this.limit = limit; }
}
