package dev.codegraph.gradle;

import java.io.Serializable;
import java.util.List;
import java.util.Map;

/** Daemon-side source model; SourceAction detaches its values before returning to the analyzer. */
public interface SourceModel extends Serializable {
    List<Map<String, Object>> getSourceSets();
}
