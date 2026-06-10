package io.oris.sdk.execution;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import okhttp3.*;

import java.io.IOException;
import java.util.Map;

public class ExecutionClient {

    private static final MediaType JSON_TYPE = MediaType.get("application/json");
    private static final ObjectMapper MAPPER = new ObjectMapper();

    private final ExecutionConfig config;
    private final OkHttpClient http;

    public ExecutionClient(ExecutionConfig config) {
        this(config, new OkHttpClient());
    }

    public ExecutionClient(ExecutionConfig config, OkHttpClient http) {
        this.config = config;
        this.http = http;
    }

    public JsonNode runJob(String threadId, Map<String, Object> input, String idempotencyKey) throws IOException {
        ObjectNode body = MAPPER.createObjectNode();
        body.put("thread_id", threadId);
        if (input != null) body.set("input", MAPPER.valueToTree(input));
        if (idempotencyKey != null) body.put("idempotency_key", idempotencyKey);
        return post("/jobs", body);
    }

    public JsonNode runJob(String threadId, Map<String, Object> input) throws IOException {
        return runJob(threadId, input, null);
    }

    public JsonNode getState(String threadId) throws IOException {
        return get("/jobs/" + threadId + "/state");
    }

    public JsonNode getDetail(String threadId) throws IOException {
        return get("/jobs/" + threadId);
    }

    public JsonNode cancel(String threadId, String reason) throws IOException {
        ObjectNode body = MAPPER.createObjectNode();
        if (reason != null) body.put("reason", reason);
        return post("/jobs/" + threadId + "/cancel", body);
    }

    public JsonNode listJobs(String status, int limit, int offset) throws IOException {
        HttpUrl.Builder urlBuilder = HttpUrl.parse(config.getBaseUrl() + "/jobs").newBuilder();
        if (status != null) urlBuilder.addQueryParameter("status", status);
        if (limit > 0) urlBuilder.addQueryParameter("limit", String.valueOf(limit));
        if (offset > 0) urlBuilder.addQueryParameter("offset", String.valueOf(offset));
        return get(urlBuilder.build());
    }

    private JsonNode post(String path, Object body) throws IOException {
        Request request = new Request.Builder()
                .url(config.getBaseUrl() + path)
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer " + config.getToken())
                .post(RequestBody.create(MAPPER.writeValueAsBytes(body), JSON_TYPE))
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("execution: " + response.code());
            JsonNode envelope = MAPPER.readTree(response.body().bytes());
            return envelope.get("data");
        }
    }

    private JsonNode get(String path) throws IOException {
        return get(HttpUrl.parse(config.getBaseUrl() + path));
    }

    private JsonNode get(HttpUrl url) throws IOException {
        Request request = new Request.Builder()
                .url(url)
                .header("Authorization", "Bearer " + config.getToken())
                .get()
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("execution: " + response.code());
            JsonNode envelope = MAPPER.readTree(response.body().bytes());
            return envelope.get("data");
        }
    }
}
