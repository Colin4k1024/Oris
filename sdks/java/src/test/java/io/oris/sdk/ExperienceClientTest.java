package io.oris.sdk;

import io.oris.sdk.experience.*;
import okhttp3.mockwebserver.MockResponse;
import okhttp3.mockwebserver.MockWebServer;
import okhttp3.mockwebserver.RecordedRequest;
import org.junit.jupiter.api.*;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class ExperienceClientTest {

    private MockWebServer server;
    private ExperienceClient client;

    private static final byte[] TEST_SEED = new byte[32];
    static { for (int i = 0; i < 32; i++) TEST_SEED[i] = (byte) (i + 1); }

    @BeforeEach
    void setUp() throws Exception {
        server = new MockWebServer();
        server.start();
        ExperienceConfig cfg = new ExperienceConfig(
                server.url("/").toString().replaceAll("/$", ""),
                "test-api-key",
                TEST_SEED,
                "agent-1"
        );
        client = new ExperienceClient(cfg);
    }

    @AfterEach
    void tearDown() throws Exception {
        server.shutdown();
    }

    @Test
    void sharePostsSignedEnvelope() throws Exception {
        server.enqueue(new MockResponse()
                .setBody("{\"gene_id\":\"g1\",\"status\":\"published\",\"published_at\":\"2026-01-01T00:00:00Z\"}")
                .setHeader("Content-Type", "application/json"));

        ShareResponse resp = client.share(Map.of("gene_id", "g1", "confidence", 0.9));
        assertEquals("g1", resp.getGeneId());
        assertEquals("published", resp.getStatus());

        RecordedRequest req = server.takeRequest();
        assertEquals("POST", req.getMethod());
        assertEquals("/experience", req.getPath());
        assertEquals("test-api-key", req.getHeader("X-Api-Key"));
        assertTrue(req.getBody().readUtf8().contains("\"sender_id\":\"agent-1\""));
    }

    @Test
    void fetchGetsExperiences() throws Exception {
        server.enqueue(new MockResponse()
                .setBody("{\"assets\":[{\"id\":\"g1\",\"confidence\":0.9}],\"next_cursor\":null}")
                .setHeader("Content-Type", "application/json"));

        FetchQuery q = new FetchQuery("timeout", 0.5, 10);
        FetchResponse resp = client.fetch(q);
        assertNotNull(resp.getAssets());
        assertEquals(1, resp.getAssets().size());
        assertEquals("g1", resp.getAssets().get(0).getId());

        RecordedRequest req = server.takeRequest();
        assertEquals("GET", req.getMethod());
        assertTrue(req.getPath().contains("q=timeout"));
        assertTrue(req.getPath().contains("min_confidence=0.5"));
    }

    @Test
    void shareReturnsErrorOnFailure() {
        server.enqueue(new MockResponse().setResponseCode(401));
        assertThrows(Exception.class, () -> client.share(Map.of("id", "x")));
    }
}
