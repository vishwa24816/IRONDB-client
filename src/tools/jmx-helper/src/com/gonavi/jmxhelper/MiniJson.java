package com.IRONDB.jmxhelper;

import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class MiniJson {
    private MiniJson() {
    }

    static Object parse(String text) {
        Parser parser = new Parser(text);
        Object value = parser.parseValue();
        parser.skipWhitespace();
        if (!parser.isDone()) {
            throw new IllegalArgumentException("JSON parsing did not consume the full payload");
        }
        return value;
    }

    static String stringify(Object value) {
        StringBuilder builder = new StringBuilder();
        writeValue(builder, value);
        return builder.toString();
    }

    private static void writeValue(StringBuilder builder, Object value) {
        if (value == null) {
            builder.append("null");
            return;
        }
        if (value instanceof String) {
            writeString(builder, (String) value);
            return;
        }
        if (value instanceof Number || value instanceof Boolean) {
            builder.append(String.valueOf(value));
            return;
        }
        if (value instanceof Map<?, ?>) {
            builder.append('{');
            Iterator<? extends Map.Entry<?, ?>> iterator = ((Map<?, ?>) value).entrySet().iterator();
            boolean first = true;
            while (iterator.hasNext()) {
                Map.Entry<?, ?> entry = iterator.next();
                if (!first) {
                    builder.append(',');
                }
                first = false;
                writeString(builder, String.valueOf(entry.getKey()));
                builder.append(':');
                writeValue(builder, entry.getValue());
            }
            builder.append('}');
            return;
        }
        if (value instanceof Iterable<?>) {
            builder.append('[');
            Iterator<?> iterator = ((Iterable<?>) value).iterator();
            boolean first = true;
            while (iterator.hasNext()) {
                if (!first) {
                    builder.append(',');
                }
                first = false;
                writeValue(builder, iterator.next());
            }
            builder.append(']');
            return;
        }
        if (value.getClass().isArray()) {
            builder.append('[');
            int length = java.lang.reflect.Array.getLength(value);
            for (int index = 0; index < length; index++) {
                if (index > 0) {
                    builder.append(',');
                }
                writeValue(builder, java.lang.reflect.Array.get(value, index));
            }
            builder.append(']');
            return;
        }
        writeString(builder, String.valueOf(value));
    }

    private static void writeString(StringBuilder builder, String value) {
        builder.append('"');
        for (int index = 0; index < value.length(); index++) {
            char ch = value.charAt(index);
            switch (ch) {
                case '"':
                    builder.append("\\\"");
                    break;
                case '\\':
                    builder.append("\\\\");
                    break;
                case '\b':
                    builder.append("\\b");
                    break;
                case '\f':
                    builder.append("\\f");
                    break;
                case '\n':
                    builder.append("\\n");
                    break;
                case '\r':
                    builder.append("\\r");
                    break;
                case '\t':
                    builder.append("\\t");
                    break;
                default:
                    if (ch < 0x20) {
                        builder.append(String.format("\\u%04x", (int) ch));
                    } else {
                        builder.append(ch);
                    }
                    break;
            }
        }
        builder.append('"');
    }

    private static final class Parser {
        private final String text;
        private int index;

        private Parser(String text) {
            this.text = text == null ? "" : text;
            this.index = 0;
        }

        private Object parseValue() {
            skipWhitespace();
            if (isDone()) {
                throw new IllegalArgumentException("unexpected end of JSON input");
            }

            char ch = text.charAt(index);
            switch (ch) {
                case '{':
                    return parseObject();
                case '[':
                    return parseArray();
                case '"':
                    return parseString();
                case 't':
                    expect("true");
                    return Boolean.TRUE;
                case 'f':
                    expect("false");
                    return Boolean.FALSE;
                case 'n':
                    expect("null");
                    return null;
                default:
                    if (ch == '-' || Character.isDigit(ch)) {
                        return parseNumber();
                    }
                    throw new IllegalArgumentException("unexpected JSON token at index " + index);
            }
        }

        private Map<String, Object> parseObject() {
            expect("{");
            LinkedHashMap<String, Object> result = new LinkedHashMap<>();
            skipWhitespace();
            if (peek('}')) {
                expect("}");
                return result;
            }
            while (true) {
                String key = parseString();
                skipWhitespace();
                expect(":");
                Object value = parseValue();
                result.put(key, value);
                skipWhitespace();
                if (peek('}')) {
                    expect("}");
                    return result;
                }
                expect(",");
            }
        }

        private List<Object> parseArray() {
            expect("[");
            ArrayList<Object> result = new ArrayList<>();
            skipWhitespace();
            if (peek(']')) {
                expect("]");
                return result;
            }
            while (true) {
                result.add(parseValue());
                skipWhitespace();
                if (peek(']')) {
                    expect("]");
                    return result;
                }
                expect(",");
            }
        }

        private String parseString() {
            expect("\"");
            StringBuilder builder = new StringBuilder();
            while (!isDone()) {
                char ch = text.charAt(index++);
                if (ch == '"') {
                    return builder.toString();
                }
                if (ch != '\\') {
                    builder.append(ch);
                    continue;
                }
                if (isDone()) {
                    throw new IllegalArgumentException("unterminated escape sequence");
                }
                char escaped = text.charAt(index++);
                switch (escaped) {
                    case '"':
                    case '\\':
                    case '/':
                        builder.append(escaped);
                        break;
                    case 'b':
                        builder.append('\b');
                        break;
                    case 'f':
                        builder.append('\f');
                        break;
                    case 'n':
                        builder.append('\n');
                        break;
                    case 'r':
                        builder.append('\r');
                        break;
                    case 't':
                        builder.append('\t');
                        break;
                    case 'u':
                        if (index + 4 > text.length()) {
                            throw new IllegalArgumentException("invalid unicode escape");
                        }
                        builder.append((char) Integer.parseInt(text.substring(index, index + 4), 16));
                        index += 4;
                        break;
                    default:
                        throw new IllegalArgumentException("unsupported escape sequence: \\" + escaped);
                }
            }
            throw new IllegalArgumentException("unterminated JSON string");
        }

        private Number parseNumber() {
            int start = index;
            if (text.charAt(index) == '-') {
                index++;
            }
            while (!isDone() && Character.isDigit(text.charAt(index))) {
                index++;
            }
            boolean isFloat = false;
            if (!isDone() && text.charAt(index) == '.') {
                isFloat = true;
                index++;
                while (!isDone() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            if (!isDone() && (text.charAt(index) == 'e' || text.charAt(index) == 'E')) {
                isFloat = true;
                index++;
                if (!isDone() && (text.charAt(index) == '+' || text.charAt(index) == '-')) {
                    index++;
                }
                while (!isDone() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            String raw = text.substring(start, index);
            return isFloat ? Double.valueOf(raw) : Long.valueOf(raw);
        }

        private void expect(String expected) {
            skipWhitespace();
            if (!text.startsWith(expected, index)) {
                throw new IllegalArgumentException("expected " + expected + " at index " + index);
            }
            index += expected.length();
        }

        private boolean peek(char ch) {
            skipWhitespace();
            return !isDone() && text.charAt(index) == ch;
        }

        private void skipWhitespace() {
            while (!isDone()) {
                char ch = text.charAt(index);
                if (!Character.isWhitespace(ch)) {
                    return;
                }
                index++;
            }
        }

        private boolean isDone() {
            return index >= text.length();
        }
    }
}
