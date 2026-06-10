package io.oris.sdk;

import io.oris.sdk.hub.*;
import okhttp3.mockwebserver.MockResponse;
import okhttp3.mockwebserver.MockWebServer;
import okhttp3.mockwebserver.RecordedRequest;
import org.junit.jupiter.api.*;

import java.util.List;

import static org.junit.jupiter.api.Assertions.*;

class HubClientTest {

    private MockWebServer server;
    private HubClient client;

    private static final byte[] TEST_SEED = new byte[32];
    static { for (int i = 0; i < 32; i++) TEST_SEED[i] = (byte) (i + 1); }

    @BeforeEach
    void setUp() throws Exception {
        server = new MockWebServer();
        server.start();
        HubConfig cfg = new HubConfig(
                server.url("/").toString().replaceAll("/$", ""),
                "test-api-key",
                TEST_SEED,
                "node-1"
        );
        client = new HubClient(cfg);
    }

    @AfterEach
    void tearDown() throws Exception {
        server.shutdown();
    }

    @Test
    void registerSendsSignedRequest() throws Exception {
        server.enqueue(new MockResponse()
                .setBody("{\"node_id\":\"node-1\",\"heartbeat_interval_seconds\":30,\"ttl_seconds\":60}")
                .setHeader("Content-Type", "application/json"));

        RegisterResponse resp = client.register("http://my-node:9000", List.of("evolve"), "0.1.0", null);
        assertEquals("node-1", resp.getNodeId());
        assertEquals(30, resp.getHeartbeatIntervalSeconds());

        RecordedRequest req = server.takeRequest();
        assertEquals("POST", req.getMethod());
        assertEquals("/hub/nodes", req.getPath());
        assertNotNull(req.getHeader("X-OEN-Signature"));
        assertTrue(req.getBody().readUtf8().contains("\"node_id\":\"node-1\""));
    }

    @Test
    void heartbeatSendsSignedPut() throws Exception {
        server.enqueue(new MockResponse()
                .setBody("{\"acknowledged\":true,\"next_heartbeat_seconds\":30}")
                .setHeader("Content-Type", "application/json"));

        HeartbeatResponse resp = client.heartbeat("active");
        assertTrue(resp.isAcknowledged());

        RecordedRequest req = server.takeRequest();
        assertEquals("PUT", req.getMethod());
        assertTrue(req.getPath().contains("/hub/nodes/node-1/heartbeat"));
        assertNotNull(req.getHeader("X-OEN-Signature"));
    }

    @Test
    void discoverUsesBearer() throws Exception {
        server.enqueue(new MockResponse()
                .setBody("{\"nodes\":[],\"total\":0}")
                .setHeader("Content-Type", "application/json"));

        DiscoveryResult resp = client.discover();
        assertEquals(0, resp.getTotal());

        RecordedRequest req = server.takeRequest();
        assertEquals("GET", req.getMethod());
        assertEquals("Bearer test-api-key", req.getHeader("Authorization"));
    }

    @Test
    void searchPostsFederatedQuery() throws Exception {
        server.enqueue(new MockResponse()
                .setBody("{\"results\":[],\"meta\":{\"nodes_queried\":0}}")
                .setHeader("Content-Type", "application/json"));

        FederatedQuery query = new FederatedQuery("error handling");
        query.setMinConfidence(0.7);
        FederatedResult resp = client.search(query);
        assertNotNull(resp.getResults());

        RecordedRequest req = server.takeRequest();
        assertEquals("POST", req.getMethod());
        assertEquals("/hub/search", req.getPath());
    }
}
