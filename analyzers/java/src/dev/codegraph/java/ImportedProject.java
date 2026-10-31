package dev.codegraph.java;

import java.util.List;

/** Build-tool-neutral immutable input to the JDT configuration boundary. Paths are repository-relative. */
record ImportedProject(List<SourceSet> sourceSets) {
    ImportedProject { sourceSets = List.copyOf(sourceSets); }
    record Compiler(String release, String source, String target, String toolchain, String encoding, boolean apiUnavailable) {
        Compiler(String release, String source, String target, String toolchain, String encoding) {
            this(release, source, target, toolchain, encoding, false);
        }
    }
    record SourceSet(String id, String moduleId, boolean test, List<String> roots,
                     List<String> visibleSourceSets, List<String> binaries,
                     Compiler compiler, boolean dependenciesComplete, List<String> modelInputs, List<String> memberFiles) {
        SourceSet(String id, String moduleId, boolean test, List<String> roots, List<String> visibleSourceSets,
                  List<String> binaries, Compiler compiler, boolean dependenciesComplete, List<String> modelInputs) {
            this(id,moduleId,test,roots,visibleSourceSets,binaries,compiler,dependenciesComplete,modelInputs,null);
        }
        SourceSet {
            roots = List.copyOf(roots);
            visibleSourceSets = List.copyOf(visibleSourceSets);
            binaries = List.copyOf(binaries);
            modelInputs = List.copyOf(modelInputs);
            memberFiles = memberFiles == null ? null : List.copyOf(memberFiles);
        }
    }
}
