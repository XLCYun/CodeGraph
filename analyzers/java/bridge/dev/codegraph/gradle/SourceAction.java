package dev.codegraph.gradle;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.gradle.tooling.BuildAction;
import org.gradle.tooling.BuildController;

/** Request only the model, then detach its data before serialization leaves the daemon. */
public final class SourceAction implements BuildAction<List<Map<String,Object>>> {
    private static final long serialVersionUID = 1L;

    @Override public List<Map<String,Object>> execute(BuildController controller) {
        List<Map<String,Object>> result = new ArrayList<>();
        for (Map<String,Object> row : controller.getModel(SourceModel.class).getSourceSets()) result.add(copyMap(row));
        return result;
    }

    private static Map<String,Object> copyMap(Map<?,?> source) {
        Map<String,Object> result = new LinkedHashMap<>();
        for (Map.Entry<?,?> entry : source.entrySet()) {
            if (!(entry.getKey() instanceof String)) throw new IllegalArgumentException("invalid model key");
            result.put((String)entry.getKey(), copy(entry.getValue()));
        }
        return result;
    }

    private static Object copy(Object value) {
        if (value == null || value instanceof String || value instanceof Boolean ||
                value instanceof Byte || value instanceof Short || value instanceof Integer || value instanceof Long ||
                value instanceof Float || value instanceof Double || value instanceof Character) return value;
        if (value instanceof Map<?,?>) return copyMap((Map<?,?>)value);
        if (value instanceof Iterable<?>) {
            List<Object> result = new ArrayList<>();
            for (Object item : (Iterable<?>)value) result.add(copy(item));
            return result;
        }
        throw new IllegalArgumentException("unsupported model value");
    }
}
