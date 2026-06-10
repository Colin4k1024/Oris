package io.oris.sdk.hub;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.oris.sdk.signing.Ed25519Signing;
import okhttp3.*;

import java.io.IOException;
import java.util.List;
import java.util.Map;

public class HubClient {

    private static final MediaType JSON_TYPE = MediaType.get("application/json");
    private static final ObjectMapper MAPPER = new ObjectMapper();

    private final HubConfig config;
    private final OkHttpClient http;

    public HubClient(HubConfig config) {
        this(config, new OkHttpClient());
    }

    public HubClient(HubConfig config, OkHttpClient http) {
        this.config = config;
        this.http = http;
    }

    public RegisterResponse register(String endpoint, List<String> capabilities, String version, String region) throws IOException {
        ObjectNode body = MAPPER.createObjectNode();
        body.put("node_id", config.getNodeId());
        body.put("endpoint", endpoint);
        body.put("public_key", Ed25519Signing.publicKeyBase64(config.getSeed()));
        ArrayNode caps = body.putArray("capabilities");
        capabilities.forEach(caps::add);
        body.put("version", version);
        if (region != null && !region.isEmpty()) body.put("region", region);

        byte[] bodyBytes = MAPPER.writeValueAsBytes(body);
        String signature = Ed25519Signing.signBody(config.getSeed(), bodyBytes);

        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/hub/nodes")
                .header("Content-Type", "application/json")
                .header("X-OEN-Signature", signature)
                .post(RequestBody.create(bodyBytes, JSON_TYPE))
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("hub register: " + response.code());
            return MAPPER.readValue(response.body().bytes(), RegisterResponse.class);
        }
    }

    public HeartbeatResponse heartbeat(String status) throws IOException {
        ObjectNode body = MAPPER.createObjectNode();
        body.put("node_id", config.getNodeId());
        if (status != null) body.put("status", status);

        byte[] bodyBytes = MAPPER.writeValueAsBytes(body);
        String signature = Ed25519Signing.signBody(config.getSeed(), bodyBytes);

        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/hub/nodes/" + config.getNodeId() + "/heartbeat")
                .header("Content-Type", "application/json")
                .header("X-OEN-Signature", signature)
                .put(RequestBody.create(bodyBytes, JSON_TYPE))
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("hub heartbeat: " + response.code());
            return MAPPER.readValue(response.body().bytes(), HeartbeatResponse.class);
        }
    }

    public DiscoveryResult discover() throws IOException {
        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/hub/nodes")
                .header("Authorization", "Bearer " + config.getApiKey())
                .get()
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("hub discover: " + response.code());
            return MAPPER.readValue(response.body().bytes(), DiscoveryResult.class);
        }
    }

    public FederatedResult search(FederatedQuery query) throws IOException {
        byte[] bodyBytes = MAPPER.writeValueAsBytes(query);

        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/hub/search")
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer " + config.getApiKey())
                .post(RequestBody.create(bodyBytes, JSON_TYPE))
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("hub search: " + response.code());
            return MAPPER.readValue(response.body().bytes(), FederatedResult.class);
        }
    }

    public JsonNode subscribe(String callbackUrl, Map<String, Object> filter) throws IOException {
        ObjectNode body = MAPPER.createObjectNode();
        body.put("subscriber_node_id", config.getNodeId());
        body.put("callback_url", callbackUrl);
        body.set("filter", MAPPER.valueToTree(filter != null ? filter : Map.of()));

        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/hub/subscriptions")
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer " + config.getApiKey())
                .post(RequestBody.create(MAPPER.writeValueAsBytes(body), JSON_TYPE))
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("hub subscribe: " + response.code());
            return MAPPER.readTree(response.body().bytes());
        }
    }

    public void unsubscribe(String subscriptionId) throws IOException {
        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/hub/subscriptions/" + subscriptionId)
                .header("Authorization", "Bearer " + config.getApiKey())
                .delete()
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("hub unsubscribe: " + response.code());
        }
    }

    public JsonNode listSubscriptions() throws IOException {
        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/hub/subscriptions")
                .header("Authorization", "Bearer " + config.getApiKey())
                .get()
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) throw new IOException("hub listSubscriptions: " + response.code());
            return MAPPER.readTree(response.body().bytes());
        }
    }
}
