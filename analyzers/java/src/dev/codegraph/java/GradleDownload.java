package dev.codegraph.java;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import static dev.codegraph.java.Main.Failure;

/** Focused wrapper transport: scoped HTTP authentication, redirects and Gradle proxy properties. */
final class GradleDownload {
    static InputStream open(URI uri, Properties wrapper, SourceSnapshot snapshot) throws IOException, Failure {
        if (uri.getScheme().equals("file")) return Files.newInputStream(Path.of(uri));
        Properties properties=new Properties();
        read(properties,snapshot.root().resolve("gradle.properties"),snapshot);
        Path user=System.getenv("GRADLE_USER_HOME")==null ? Path.of(System.getProperty("user.home"),".gradle") : Path.of(System.getenv("GRADLE_USER_HOME"));
        read(properties,user.resolve("gradle.properties"),snapshot);
        if(properties.containsKey("systemProp.socksProxyHost")) throw new Failure("unsupported_configuration","SOCKS Gradle distribution proxy unsupported");
        int timeout;
        try { timeout=Integer.parseInt(wrapper.getProperty("networkTimeout","10000")); }
        catch(NumberFormatException failure) { throw new Failure("unsupported_configuration","invalid Gradle wrapper network timeout"); }
        if(timeout<=0 || timeout>120000) throw new Failure("unsupported_configuration","unsupported Gradle wrapper network timeout");
        if(uri.getHost()==null) throw new Failure("unsupported_configuration","invalid Gradle distribution host");
        URI origin=uri;
        String authorization=authorization(properties,origin);
        for(int redirect=0;redirect<=5;redirect++) {
            if(uri.getHost()==null || !Set.of("http","https").contains(uri.getScheme())) throw new Failure("unsupported_configuration","unsupported Gradle distribution transport");
            String protocol=uri.getScheme();
            String host=properties.getProperty("systemProp."+protocol+".proxyHost");
            int port=integer(properties.getProperty("systemProp."+protocol+".proxyPort",protocol.equals("https")?"443":"80"));
            String proxyUser=properties.getProperty("systemProp."+protocol+".proxyUser");
            String proxyPassword=properties.getProperty("systemProp."+protocol+".proxyPassword","");
            String excluded=properties.getProperty("systemProp.http.nonProxyHosts","");
            if(host==null) {
                String hint=System.getenv(protocol.toUpperCase(Locale.ROOT)+"_PROXY");
                if(hint==null) hint=System.getenv(protocol+"_proxy");
                if(hint!=null && !hint.isBlank()) {
                    URI proxy=URI.create(hint);
                    if(!"http".equals(proxy.getScheme())) throw new Failure("unsupported_configuration","unsupported Gradle proxy transport");
                    host=proxy.getHost(); port=proxy.getPort()==-1?80:proxy.getPort();
                    if(proxy.getUserInfo()!=null) { String[] pair=proxy.getUserInfo().split(":",2); proxyUser=pair[0];proxyPassword=pair.length==2?pair[1]:""; }
                    excluded=System.getenv().getOrDefault("NO_PROXY",System.getenv().getOrDefault("no_proxy","")).replace(',', '|');
                }
            }
            String targetHost=uri.getHost();
            boolean bypass=Arrays.stream(excluded.split("\\|")).anyMatch(pattern -> matches(targetHost,pattern));
            Proxy proxy=host==null || bypass ? Proxy.NO_PROXY : new Proxy(Proxy.Type.HTTP,new InetSocketAddress(host,port));
            HttpURLConnection connection=(HttpURLConnection)uri.toURL().openConnection(proxy);
            connection.setInstanceFollowRedirects(false); connection.setConnectTimeout(timeout); connection.setReadTimeout(timeout);
            if(sameOrigin(origin,uri) && authorization!=null) connection.setRequestProperty("Authorization",authorization);
            if(proxy!=Proxy.NO_PROXY && proxyUser!=null) {
                final String scopedHost=host,userName=proxyUser,password=proxyPassword;
                final int scopedPort=port;
                connection.setAuthenticator(new Authenticator() {
                    protected PasswordAuthentication getPasswordAuthentication() {
                        return getRequestorType()==RequestorType.PROXY && scopedHost.equalsIgnoreCase(getRequestingHost()) && scopedPort==getRequestingPort()
                            ? new PasswordAuthentication(userName,password.toCharArray()) : null;
                    }
                });
            }
            int status;
            try { status=connection.getResponseCode(); }
            catch(IOException | RuntimeException failure) { connection.disconnect(); throw failure; }
            if(Set.of(301,302,303,307,308).contains(status)) {
                String location=connection.getHeaderField("Location"); connection.disconnect();
                if(location==null) throw new Failure("unsupported_configuration","invalid Gradle distribution redirect");
                URI next=uri.resolve(location);
                if(uri.getScheme().equals("https") && !next.getScheme().equals("https"))
                    throw new Failure("unsupported_configuration","insecure Gradle distribution redirect");
                uri=next; continue;
            }
            if(status!=200) { connection.disconnect(); throw new Failure("unsupported_configuration","Gradle distribution server rejected download"); }
            InputStream body;
            try { body=connection.getInputStream(); }
            catch(IOException failure) { connection.disconnect(); throw failure; }
            return new FilterInputStream(body) {
                public void close() throws IOException { try { super.close(); } finally { connection.disconnect(); } }
            };
        }
        throw new Failure("unsupported_configuration","too many Gradle distribution redirects");
    }
    private static boolean matches(String host,String pattern) {
        if(pattern.isBlank()) return false;
        if(pattern.startsWith(".")) return host.endsWith(pattern) || host.equals(pattern.substring(1));
        return host.matches("(?i)"+java.util.regex.Pattern.quote(pattern).replace("*","\\E.*\\Q"));
    }
    private static int integer(String value) throws Failure {
        try { int port=Integer.parseInt(value); if(port>0 && port<65536) return port; }
        catch(NumberFormatException ignored) { }
        throw new Failure("unsupported_configuration","invalid Gradle proxy port");
    }
    private static boolean sameOrigin(URI a,URI b) {
        return Objects.equals(a.getScheme(),b.getScheme()) && Objects.equals(a.getHost(),b.getHost()) && a.getPort()==b.getPort();
    }
    private static void read(Properties values,Path file,SourceSnapshot snapshot) throws IOException,Failure {
        snapshot.watch(file);
        if(Files.exists(file)) try(var input=Files.newInputStream(file)) { values.load(input); }
    }
    private static String authorization(Properties properties,URI uri) {
        String prefix="systemProp.gradle."+uri.getHost().toLowerCase(Locale.ROOT).replace('.','_')+".";
        String token=properties.getProperty(prefix+"wrapperToken");
        if(token!=null) return "Bearer "+token;
        String user=properties.getProperty(prefix+"wrapperUser",properties.getProperty("systemProp.gradle.wrapperUser"));
        String password=properties.getProperty(prefix+"wrapperPassword",properties.getProperty("systemProp.gradle.wrapperPassword",""));
        if(user==null && uri.getUserInfo()!=null) { String[] pair=uri.getUserInfo().split(":",2); user=pair[0];password=pair.length==2?pair[1]:""; }
        return user==null?null:"Basic "+Base64.getEncoder().encodeToString((user+":"+password).getBytes(StandardCharsets.UTF_8));
    }
}
