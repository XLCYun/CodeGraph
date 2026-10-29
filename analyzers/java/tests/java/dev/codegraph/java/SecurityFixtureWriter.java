package dev.codegraph.java;

import java.nio.file.*;
import org.sonatype.plexus.components.cipher.DefaultPlexusCipher;

/** Creates synthetic encrypted credentials; never prints their contents. */
public final class SecurityFixtureWriter {
    public static void main(String[] args) throws Exception {
        Path home = Path.of(args[0]);
        Files.createDirectories(home.resolve(".m2"));
        var cipher = new DefaultPlexusCipher();
        Files.writeString(home.resolve("master.xml"), "<settingsSecurity><master>" +
            cipher.encryptAndDecorate("fixture-master", "settings.security") + "</master></settingsSecurity>");
        Files.writeString(home.resolve("servers.xml"), "<servers><server><id>fixture</id><username>fixture-user</username><password>" +
            cipher.encryptAndDecorate("fixture-password", "fixture-master") + "</password></server></servers>");
        Files.writeString(home.resolve(".m2/settings-security.xml"), "<settingsSecurity><relocation>" +
            home.resolve("master.xml") + "</relocation></settingsSecurity>");
    }
}
