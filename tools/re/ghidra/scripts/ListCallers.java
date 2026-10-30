// Печатает вызывающие функции для списка адресов.
// Аргумент: <addr1,addr2,...>
// @category AJ199
// @runtime Java

import java.util.LinkedHashMap;
import java.util.Map;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;

public class ListCallers extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            throw new IllegalArgumentException("usage: ListCallers <addr1,addr2,...>");
        }
        for (String token : args[0].split(",")) {
            String text = token.trim();
            if (text.isEmpty()) {
                continue;
            }
            Address target = toAddr(Long.parseLong(text.replaceFirst("^0[xX]", ""), 16));
            println("TARGET " + target);
            Map<String, String> callers = new LinkedHashMap<>();
            for (Reference ref : getReferencesTo(target)) {
                Address from = ref.getFromAddress();
                Function function = getFunctionContaining(from);
                String key = function != null ? function.getEntryPoint() + " " + function.getName() : "-";
                callers.putIfAbsent(key, from.toString());
            }
            for (Map.Entry<String, String> entry : callers.entrySet()) {
                println("  " + entry.getKey() + " first_call=" + entry.getValue());
            }
        }
    }
}
