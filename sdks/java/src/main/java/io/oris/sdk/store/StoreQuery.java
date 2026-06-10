package io.oris.sdk.store;

public class StoreQuery {
    private String q;
    private String taskClass;
    private double minConfidence;
    private String source;
    private String orderBy;
    private int limit;
    private int offset;

    public StoreQuery() {}

    public String getQ() { return q; }
    public void setQ(String q) { this.q = q; }
    public String getTaskClass() { return taskClass; }
    public void setTaskClass(String taskClass) { this.taskClass = taskClass; }
    public double getMinConfidence() { return minConfidence; }
    public void setMinConfidence(double minConfidence) { this.minConfidence = minConfidence; }
    public String getSource() { return source; }
    public void setSource(String source) { this.source = source; }
    public String getOrderBy() { return orderBy; }
    public void setOrderBy(String orderBy) { this.orderBy = orderBy; }
    public int getLimit() { return limit; }
    public void setLimit(int limit) { this.limit = limit; }
    public int getOffset() { return offset; }
    public void setOffset(int offset) { this.offset = offset; }
}
