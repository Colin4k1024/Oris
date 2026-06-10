package io.oris.sdk.hub;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.databind.JsonNode;

import java.util.List;

@JsonIgnoreProperties(ignoreUnknown = true)
public class FederatedResult {
    @JsonProperty("results") private List<JsonNode> results;
    @JsonProperty("meta") private JsonNode meta;

    public List<JsonNode> getResults() { return results; }
    public void setResults(List<JsonNode> results) { this.results = results; }
    public JsonNode getMeta() { return meta; }
    public void setMeta(JsonNode meta) { this.meta = meta; }
}
