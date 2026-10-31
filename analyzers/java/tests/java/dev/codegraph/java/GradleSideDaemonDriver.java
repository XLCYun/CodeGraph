package dev.codegraph.java;

import java.io.File;
import java.io.OutputStream;
import org.gradle.tooling.GradleConnector;
import org.gradle.tooling.model.GradleProject;

/** A separate user's idle daemon must survive the product's owned-tool cleanup. */
public final class GradleSideDaemonDriver {
    public static void main(String[] args) throws Exception {
        var connector=GradleConnector.newConnector().forProjectDirectory(new File(args[0]))
            .useInstallation(new File(args[1])).useGradleUserHomeDir(new File(args[2]));
        try(var connection=connector.connect()) {
            connection.model(GradleProject.class).setJavaHome(new File(args[3]))
                .setStandardOutput(OutputStream.nullOutputStream()).setStandardError(OutputStream.nullOutputStream()).get();
            java.nio.file.Files.writeString(java.nio.file.Path.of(args[4]),"ready");
            System.in.read();
        } finally {
            connector.disconnect();
            org.gradle.tooling.internal.consumer.DefaultGradleConnector.close();
        }
    }
}
