// Собирает строки-якоря, ссылки на них, HID-импорты и декомпилирует
// функции, связанные с протоколом AJAZZ AJ199.
// @category AJ199
// @runtime Java

import java.io.File;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolIterator;

public class DumpAj199Anchors extends GhidraScript {
    private static final String[] KEYWORDS = {
        "SendCMD", "SendData", "WaitNotify", "GetData", "ReadROM", "ReadEEPROM",
        "WriteEEPROM", "CheckPsd", "GetConnectStatus", "GetPower", "GetDevVersion",
        "GetVersionNumber", "FindDev", "Psd_Thread", "ServiceThread", "OnDeviceChange",
        "Apply", "ReadCurCfg", "GetProfile", "SetProfile", "SetCurProfile",
        "dwDpi", "nHwDpi", "bMul", "DPI", "LOD", "Ripple", "Debounce", "Sleep",
        "EEPROM_To_", "keyinfo", "HdMacro_To_StMacro", "StMacro_To_HdMacro",
        "macro", "Profile", "CID", "MID", "Power", "battery", "charging",
        "HidD_", "HidP_", "Report", "feature", "notify", "timeout", "TimeOut"
    };

    private static final String[] HID_IMPORTS = {
        "HidD_SetFeature", "HidD_GetAttributes", "HidD_GetHidGuid",
        "HidD_GetPreparsedData", "HidP_GetCaps", "HidP_GetSpecificButtonCaps",
        "HidP_GetSpecificValueCaps", "HidD_FreePreparsedData"
    };

    private static final String[] CLASS_NAMES = {
        "CUSBDriver", "CMouseManager", "CMacroDlg", "CPickMacroDlg"
    };

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = new File(args.length > 0 ? args[0] : "aj199_ghidra_out");
        File decompDir = new File(outDir, "decompiled");
        outDir.mkdirs();
        decompDir.mkdirs();

        Listing listing = currentProgram.getListing();
        Set<Function> targets = new LinkedHashSet<>();
        List<String[]> anchorRows = new ArrayList<>();

        for (Data data : listing.getDefinedData(true)) {
            if (data == null || !data.hasStringValue()) {
                continue;
            }
            Object raw = data.getValue();
            if (!(raw instanceof String)) {
                continue;
            }
            String value = (String) raw;
            if (!matches(value)) {
                continue;
            }

            List<String> refs = new ArrayList<>();
            for (Reference ref : getReferencesTo(data.getAddress())) {
                Address from = ref.getFromAddress();
                Function func = getFunctionContaining(from);
                String funcName = func != null ? func.getName() : "-";
                String funcEntry = func != null ? func.getEntryPoint().toString() : "-";
                refs.add(from + " -> " + funcEntry + " " + funcName);
                if (func != null) {
                    targets.add(func);
                }
            }
            anchorRows.add(new String[] {
                data.getAddress().toString(), data.getDataType().getName(), value,
                String.join(" | ", refs)
            });
        }

        for (String importName : HID_IMPORTS) {
            List<Symbol> symbols = new ArrayList<>();
            SymbolIterator iterator = currentProgram.getSymbolTable().getSymbols(importName);
            while (iterator.hasNext()) {
                symbols.add(iterator.next());
            }
            for (Symbol symbol : symbols) {
                for (Reference ref : getReferencesTo(symbol.getAddress())) {
                    Function func = getFunctionContaining(ref.getFromAddress());
                    if (func != null) {
                        targets.add(func);
                        anchorRows.add(new String[] {
                            symbol.getAddress().toString(), "HID_IMPORT", importName,
                            ref.getFromAddress() + " -> " + func.getEntryPoint() + " " + func.getName()
                        });
                    }
                }
            }
        }

        List<Symbol> classSymbols = new ArrayList<>();
        for (String className : CLASS_NAMES) {
            SymbolIterator iterator = currentProgram.getSymbolTable().getSymbols(className);
            while (iterator.hasNext()) {
                classSymbols.add(iterator.next());
            }
        }
        for (Symbol symbol : classSymbols) {
            for (Reference ref : getReferencesTo(symbol.getAddress())) {
                Function func = getFunctionContaining(ref.getFromAddress());
                if (func != null) {
                    targets.add(func);
                }
            }
        }

        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);

        List<Function> ordered = new ArrayList<>(targets);
        ordered.sort(Comparator.comparing(f -> f.getEntryPoint().toString()));

        try (PrintWriter anchors = new PrintWriter(new File(outDir, "anchors.txt"));
             PrintWriter functions = new PrintWriter(new File(outDir, "functions.txt"))) {
            for (String[] row : anchorRows) {
                anchors.println(row[0] + "\t" + row[1] + "\t" + row[2] + "\t" + row[3]);
            }

            for (Function func : ordered) {
                String entry = func.getEntryPoint().toString();
                functions.println(entry + "\t" + func.getName() + "\t" + func.getSignature());

                DecompileResults result = decompiler.decompileFunction(func, 90, monitor);
                if (result == null || !result.decompileCompleted()) {
                    functions.println("\tDECOMPILE_FAILED\t" + (result != null ? result.getErrorMessage() : "null"));
                    continue;
                }
                String c = result.getDecompiledFunction().getC();
                String safeName = func.getName().replaceAll("[^A-Za-z0-9_.-]", "_");
                File target = new File(decompDir, entry + "_" + safeName + ".c");
                try (PrintWriter writer = new PrintWriter(target)) {
                    writer.println("// " + entry + " " + func.getName());
                    writer.println("// " + func.getSignature());
                    writer.println();
                    writer.println(c);
                }
            }
        } finally {
            decompiler.dispose();
        }

        println("AJ199 anchors: " + anchorRows.size());
        println("AJ199 target functions: " + ordered.size());
        println("Output: " + outDir.getAbsolutePath());
    }

    private boolean matches(String value) {
        String lower = value.toLowerCase(Locale.ROOT);
        for (String keyword : KEYWORDS) {
            if (lower.contains(keyword.toLowerCase(Locale.ROOT))) {
                return true;
            }
        }
        return false;
    }
}
