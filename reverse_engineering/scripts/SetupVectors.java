import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.lang.*;
import ghidra.program.model.mem.*;
import java.math.BigInteger;
public class SetupVectors extends GhidraScript {
  public void run() throws Exception {
    Memory mem = currentProgram.getMemory();
    Address base = toAddr(0x08020000L);
    Register tmode = currentProgram.getRegister("TMode");
    try { setAnalysisOption(currentProgram, "ARM Aggressive Instruction Finder", "true"); } catch (Exception e) { println("opt fail "+e); }
    for (int i = 1; i < 120; i++) {
      long v = mem.getInt(base.add(i*4)) & 0xffffffffL;
      if (v == 0) continue;
      if ((v & 1) == 1 && v >= 0x08020000L && v < 0x08060000L) {
        Address a = toAddr(v & ~1L);
        currentProgram.getProgramContext().setValue(tmode, a, a, BigInteger.ONE);
        disassemble(a);
        createFunction(a, i==1 ? "Reset_Handler" : String.format("vec_%d", i));
      }
    }
    // Create RAM blocks so references resolve
    try { mem.createUninitializedBlock("SRAM", toAddr(0x20000000L), 0x80000, false); } catch (Exception e) { println("sram "+e); }
    try { mem.createUninitializedBlock("PERIPH", toAddr(0x40000000L), 0x20000000L, false); } catch (Exception e) { println("periph "+e); }
    try { mem.createUninitializedBlock("EXTRAM", toAddr(0xC0000000L), 0x2000000L, false); } catch (Exception e) { println("ext "+e); }
    try { mem.createUninitializedBlock("SCS", toAddr(0xE0000000L), 0x100000L, false); } catch (Exception e) { println("scs "+e); }
  }
}
