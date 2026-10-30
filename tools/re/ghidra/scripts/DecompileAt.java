// Декомпилирует функции по списку адресов.
// Аргументы: <output-dir> <addr1,addr2,...>
// @category AJ199
// @runtime Java

import java.io.File;
import java.io.PrintWriter;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class DecompileAt extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            throw new IllegalArgumentException("usage: DecompileAt <outDir> <addr1,addr2,...>");
        }
        File outDir = new File(args[0]);
        outDir.mkdirs();

        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);

        try {
            for (String token : args[1].split(",")) {
                String text = token.trim();
                if (text.isEmpty()) {
                    continue;
                }
                Address address = toAddr(Long.parseLong(text.replaceFirst("^0[xX]", ""), 16));
                Function function = getFunctionAt(address);
                if (function == null) {
                    function = getFunctionContaining(address);
                }
                if (function == null) {
                    println("No function at " + text);
                    continue;
                }
                DecompileResults result = decompiler.decompileFunction(function, 120, monitor);
                if (result == null || !result.decompileCompleted()) {
                    println("Decompile failed at " + text + ": " + (result != null ? result.getErrorMessage() : "null"));
                    continue;
                }
                String entry = function.getEntryPoint().toString();
                String safeName = function.getName().replaceAll("[^A-Za-z0-9_.-]", "_");
                File target = new File(outDir, entry + "_" + safeName + ".c");
                try (PrintWriter writer = new PrintWriter(target)) {
                    writer.println("// " + entry + " " + function.getName());
                    writer.println("// " + function.getSignature());
                    writer.println();
                    writer.println(result.getDecompiledFunction().getC());
                }
                println(entry + " -> " + target.getName());
            }
        } finally {
            decompiler.dispose();
        }
    }
}
