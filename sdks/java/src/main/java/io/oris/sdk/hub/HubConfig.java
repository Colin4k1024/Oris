package io.oris.sdk.hub;

public class HubConfig {
    private final String baseUrl;
    private final String apiKey;
    private final byte[] seed;
    private final String nodeId;

    public HubConfig(String baseUrl, String apiKey, byte[] seed, String nodeId) {
        this.baseUrl = baseUrl;
        this.apiKey = apiKey;
        this.seed = seed.clone();
        this.nodeId = nodeId;
    }

    public String getBaseUrl() { return baseUrl; }
    public String getApiKey() { return apiKey; }
    public byte[] getSeed() { return seed.clone(); }
    public String getNodeId() { return nodeId; }
}
