package io.oris.sdk.experience;

public class ExperienceConfig {
    private final String baseUrl;
    private final String apiKey;
    private final byte[] seed;
    private final String senderId;

    public ExperienceConfig(String baseUrl, String apiKey, byte[] seed, String senderId) {
        this.baseUrl = baseUrl;
        this.apiKey = apiKey;
        this.seed = seed.clone();
        this.senderId = senderId;
    }

    public String getBaseUrl() { return baseUrl; }
    public String getApiKey() { return apiKey; }
    public byte[] getSeed() { return seed.clone(); }
    public String getSenderId() { return senderId; }
}
