package io.oris.sdk.sync;

import java.util.ArrayList;
import java.util.List;

public class PushResult {
    private int pushed;
    private int failed;
    private List<PushError> errors = new ArrayList<>();

    public int getPushed() { return pushed; }
    public void setPushed(int pushed) { this.pushed = pushed; }
    public int getFailed() { return failed; }
    public void setFailed(int failed) { this.failed = failed; }
    public List<PushError> getErrors() { return errors; }
    public void setErrors(List<PushError> errors) { this.errors = errors; }
}
