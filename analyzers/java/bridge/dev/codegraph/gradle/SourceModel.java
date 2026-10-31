package dev.codegraph.gradle;

import java.io.Serializable;
import java.util.List;
import java.util.Map;

/** Only bootstrap collection/value types cross the daemon JVM boundary. */
public interface SourceModel extends Serializable {
    List<Map<String, Object>> getSourceSets();
}
