package io.oris.sdk.experience;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.oris.sdk.signing.Ed25519Signing;
import okhttp3.*;

import java.io.IOException;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.*;

public class ExperienceClient {

    private static final MediaType JSON_TYPE = MediaType.get("application/json");
    private static final ObjectMapper MAPPER = new ObjectMapper();

    private final ExperienceConfig config;
    private final OkHttpClient http;

    public ExperienceClient(ExperienceConfig config) {
        this(config, new OkHttpClient());
    }

    public ExperienceClient(ExperienceConfig config, OkHttpClient http) {
        this.config = config;
        this.http = http;
    }

    public ShareResponse share(Object payload) throws IOException {
        String signature = Ed25519Signing.signPayload(config.getSeed(), payload);
        String timestamp = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss'Z'")
                .withZone(ZoneOffset.UTC)
                .format(Instant.now());

        ObjectNode envelope = MAPPER.createObjectNode();
        envelope.put("sender_id", config.getSenderId());
        envelope.put("message_type", "publish");
        envelope.set("payload", MAPPER.valueToTree(payload));
        envelope.put("signature", signature);
        envelope.put("timestamp", timestamp);

        ObjectNode body = MAPPER.createObjectNode();
        body.set("envelope", envelope);

        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/experience")
                .header("Content-Type", "application/json")
                .header("X-Api-Key", config.getApiKey())
                .post(RequestBody.create(MAPPER.writeValueAsBytes(body), JSON_TYPE))
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) {
                throw new IOException("experience share: " + response.code());
            }
            return MAPPER.readValue(response.body().bytes(), ShareResponse.class);
        }
    }

    public FetchResponse fetch(FetchQuery query) throws IOException {
        HttpUrl.Builder urlBuilder = HttpUrl.parse(config.getBaseUrl() + "/experience").newBuilder();
        if (query != null) {
            if (query.getQ() != null) urlBuilder.addQueryParameter("q", query.getQ());
            if (query.getMinConfidence() > 0) urlBuilder.addQueryParameter("min_confidence", String.valueOf(query.getMinConfidence()));
            if (query.getLimit() > 0) urlBuilder.addQueryParameter("limit", String.valueOf(query.getLimit()));
            if (query.getCursor() != null) urlBuilder.addQueryParameter("cursor", query.getCursor());
        }

        Request request = new Request.Builder()
                .url(urlBuilder.build())
                .get()
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) {
                throw new IOException("experience fetch: " + response.code());
            }
            return MAPPER.readValue(response.body().bytes(), FetchResponse.class);
        }
    }

    public void registerPublicKey() throws IOException {
        String hex = Ed25519Signing.publicKeyHex(config.getSeed());
        ObjectNode body = MAPPER.createObjectNode();
        body.put("sender_id", config.getSenderId());
        body.put("public_key_hex", hex);

        Request request = new Request.Builder()
                .url(config.getBaseUrl() + "/public-keys")
                .header("Content-Type", "application/json")
                .header("X-Api-Key", config.getApiKey())
                .post(RequestBody.create(MAPPER.writeValueAsBytes(body), JSON_TYPE))
                .build();

        try (Response response = http.newCall(request).execute()) {
            if (!response.isSuccessful()) {
                throw new IOException("experience registerPublicKey: " + response.code());
            }
        }
    }
}
