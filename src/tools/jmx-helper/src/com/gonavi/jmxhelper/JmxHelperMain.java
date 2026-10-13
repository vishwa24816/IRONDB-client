package com.IRONDB.jmxhelper;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.Map;

public final class JmxHelperMain {
    private JmxHelperMain() {
    }

    public static void main(String[] args) throws Exception {
        Map<String, Object> response = new LinkedHashMap<>();
        try {
            Map<String, Object> request = readRequest(System.in);
            response.putAll(JmxRuntime.handle(request));
            response.put("ok", Boolean.TRUE);
        } catch (Throwable error) {
            response.clear();
            response.put("ok", Boolean.FALSE);
            response.put("error", error.getMessage() == null ? error.getClass().getName() : error.getMessage());
            Map<String, Object> details = new LinkedHashMap<>();
            details.put("exception", error.getClass().getName());
            response.put("details", details);
        }
        System.out.print(MiniJson.stringify(response));
        System.out.flush();
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> readRequest(InputStream inputStream) throws Exception {
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        byte[] chunk = new byte[4096];
        int read;
        while ((read = inputStream.read(chunk)) >= 0) {
            buffer.write(chunk, 0, read);
        }
        String payload = new String(buffer.toByteArray(), StandardCharsets.UTF_8);
        Object parsed = MiniJson.parse(payload);
        if (!(parsed instanceof Map<?, ?>)) {
            throw new IllegalArgumentException("helper request must be a JSON object");
        }
        return (Map<String, Object>) parsed;
    }
}
