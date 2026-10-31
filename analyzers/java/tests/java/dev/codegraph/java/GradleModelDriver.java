package dev.codegraph.java;

import java.nio.file.*;
import java.util.List;

/** Real-tool diagnostics and deadline harness; never packaged. */
public final class GradleModelDriver {
    public static void main(String[] args) throws Exception {
        long deadline=System.nanoTime()+Long.parseLong(args[1])*1_000_000L;
        ProjectModel.Checkpoint check=() -> {
            if(System.nanoTime()>deadline) throw new Main.Failure("resource_limit","analysis timeout");
        };
        try(var snapshot=SourceSnapshot.capture(Path.of(args[0]).toRealPath(),List.of(),check)) {
            try { System.out.println(GradleProjectImporter.load(snapshot,SdkDiscovery.discover(),check)); }
            catch(Main.Failure e) {
                System.out.println(e.code+": "+e.getMessage());
                Path log=snapshot.outputRoot().resolve("gradle-output.log");
                if(Files.exists(log)) System.out.println(Files.readString(log));
                throw e;
            }
        }
    }
}
