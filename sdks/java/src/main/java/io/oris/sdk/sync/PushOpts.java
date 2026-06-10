package io.oris.sdk.sync;

import java.util.List;

public class PushOpts {
    private List<String> geneIds;

    public PushOpts() {}
    public PushOpts(List<String> geneIds) { this.geneIds = geneIds; }

    public List<String> getGeneIds() { return geneIds; }
    public void setGeneIds(List<String> geneIds) { this.geneIds = geneIds; }
}
