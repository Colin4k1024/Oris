package io.oris.sdk.execution;

public class ExecutionConfig {
    private final String baseUrl;
    private final String token;

    public ExecutionConfig(String baseUrl, String token) {
        this.baseUrl = baseUrl;
        this.token = token;
    }

    public String getBaseUrl() { return baseUrl; }
    public String getToken() { return token; }
}
