package io.oris.sdk.hub;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.databind.JsonNode;

import java.util.List;

@JsonIgnoreProperties(ignoreUnknown = true)
public class DiscoveryResult {
    @JsonProperty("nodes") private List<JsonNode> nodes;
    @JsonProperty("total") private int total;

    public List<JsonNode> getNodes() { return nodes; }
    public void setNodes(List<JsonNode> nodes) { this.nodes = nodes; }
    public int getTotal() { return total; }
    public void setTotal(int total) { this.total = total; }
}
