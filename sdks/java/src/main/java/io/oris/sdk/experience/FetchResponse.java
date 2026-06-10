package io.oris.sdk.experience;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.List;

@JsonIgnoreProperties(ignoreUnknown = true)
public class FetchResponse {
    @JsonProperty("assets") private List<NetworkAsset> assets;
    @JsonProperty("next_cursor") private String nextCursor;

    public List<NetworkAsset> getAssets() { return assets; }
    public void setAssets(List<NetworkAsset> assets) { this.assets = assets; }
    public String getNextCursor() { return nextCursor; }
    public void setNextCursor(String nextCursor) { this.nextCursor = nextCursor; }
}
