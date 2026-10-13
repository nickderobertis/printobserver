using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Net;
using System.Runtime.InteropServices;
using System.Threading;

// Loaded once by the test-only PowerShell worker. The controller rotates a
// flushed ETL file before decoding it: ProcessTrace sees a closed, finite file.
public sealed class PrintobserverTrace : IDisposable
{
    const string Name = "printobserver-kernel-observation-v1";
    const uint ProcessFlag = 1;
    const uint SystemLogger = 0x02000000, Sequential = 1;
    static readonly Guid ProcessProvider = new Guid("22fb2cd6-0e7b-422b-a0c7-2fad1fd0e716");
    static readonly Guid RequestProvider = new Guid("2f07e2ee-15db-40f1-90ef-9d7ba282188a");
    readonly Mutex mutex = new Mutex(false, "Global\\printobserver-kernel-observation-v1");
    readonly string directory;
    readonly Dictionary<uint, Lifetime> alive = new Dictionary<uint, Lifetime>();
    readonly HashSet<string> endpoints = new HashSet<string>();
    ulong controller;
    int chunk;
    bool held, disposed, rootSeen, rootEnded;
    uint root;
    long boundary;
    Exception decodingError;
    long sequence, invocation;
    bool invalidateNext;

    sealed class Lifetime { public long Identity, Root; }
    sealed class Observation { public long Time; public uint Pid, Parent; public int Kind; public string Address; }

    // llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] These declarations consume the external Microsoft SDK ABI, cited beside each type and signature. Exact size and field-offset checks run on x64 and ARM64 before real native calls. Generating another interop copy would duplicate this boundary rather than give the kernel ABI an in-repository source.
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/wmistr.h
    [StructLayout(LayoutKind.Sequential)] struct Wnode { public uint Size, Provider; public ulong Context; public long Time; public Guid Guid; public uint Clock, Flags; }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [StructLayout(LayoutKind.Sequential)] struct Properties {
        public Wnode Wnode;
        public uint BufferSize, MinimumBuffers, MaximumBuffers, MaximumFileSize, LogFileMode, FlushTimer, EnableFlags;
        public int AgeLimit;
        public uint NumberOfBuffers, FreeBuffers, EventsLost, BuffersWritten, LogBuffersLost, RealTimeBuffersLost;
        public IntPtr LoggerThread;
        public uint LogFileNameOffset, LoggerNameOffset;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntprov.h
    [StructLayout(LayoutKind.Sequential)] struct Descriptor { public ushort Id; public byte Version, Channel, Level, Opcode; public ushort Task; public ulong Keyword; }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/um/evntcons.h
    [StructLayout(LayoutKind.Sequential)] struct Header {
        public ushort Size, Type, Flags, Property;
        public uint Thread, Process;
        public long Time;
        public Guid Provider;
        public Descriptor Descriptor;
        public ulong ProcessorTime;
        public Guid Activity;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/um/evntcons.h
    [StructLayout(LayoutKind.Sequential)] struct Record {
        public Header Header;
        public uint BufferContext;
        public ushort ExtendedCount, DataLength;
        public IntPtr ExtendedData, Data, Context;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [StructLayout(LayoutKind.Sequential)] struct EventHeader {
        public ushort Size, FieldType;
        public uint Version, Thread, Process;
        public long Time;
        public Guid Guid;
        public ulong ProcessorTime;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [StructLayout(LayoutKind.Sequential)] struct Event {
        public EventHeader Header;
        public uint Instance, ParentInstance;
        public Guid Parent;
        public IntPtr Data;
        public uint Length, Context;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/um/timezoneapi.h
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct TimeZone {
        public int Bias;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string StandardName;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst=8)] public ushort[] StandardDate;
        public int StandardBias;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string DaylightName;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst=8)] public ushort[] DaylightDate;
        public int DaylightBias;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [StructLayout(LayoutKind.Sequential)] struct FileHeader {
        public uint BufferSize, Version, ProviderVersion, Processors;
        public long EndTime;
        public uint TimerResolution, MaximumFileSize, Mode, BuffersWritten;
        public Guid Instance;
        public IntPtr LoggerName, FileName;
        public TimeZone Zone;
        public long BootTime, Frequency, StartTime;
        public uint Flags, BuffersLost;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct LogFile {
        public IntPtr FileName, LoggerName;
        public long CurrentTime;
        public uint BuffersRead, Mode;
        public Event CurrentEvent;
        public FileHeader Header;
        public IntPtr BufferCallback;
        public uint BufferSize, Filled, EventsLost;
        public IntPtr EventCallback;
        public uint Kernel;
        public IntPtr Context;
    }
    // SDK declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/um/tdh.h
    [StructLayout(LayoutKind.Sequential)] struct PropertyData { public ulong Name; public uint Index, Reserved; }
    // SDK callback declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [UnmanagedFunctionPointer(CallingConvention.Winapi)] delegate void EventCallback(IntPtr record);
    // SDK callback declaration: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [UnmanagedFunctionPointer(CallingConvention.Winapi)] delegate uint BufferCallback(IntPtr logfile);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern uint StartTraceW(out ulong handle, string name, IntPtr properties);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern uint ControlTraceW(ulong handle, string name, IntPtr properties, uint code);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [DllImport("advapi32.dll")] static extern uint EnableTraceEx2(ulong handle, ref Guid provider, uint control, byte level, ulong any, ulong all, uint timeout, IntPtr parameters);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern ulong OpenTraceW(ref LogFile file);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [DllImport("advapi32.dll")] static extern uint ProcessTrace([In] ulong[] handles, uint count, IntPtr start, IntPtr end);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/evntrace.h
    [DllImport("advapi32.dll")] static extern uint CloseTrace(ulong handle);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/um/tdh.h
    [DllImport("tdh.dll")] static extern uint TdhGetPropertySize(IntPtr record, uint contexts, IntPtr context, uint count, ref PropertyData property, out uint size);
    // SDK function signature: https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/um/tdh.h
    [DllImport("tdh.dll")] static extern uint TdhGetProperty(IntPtr record, uint contexts, IntPtr context, uint count, ref PropertyData property, uint size, [Out] byte[] data);

    // llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

    static void Require(uint code, string operation) {
        if (code != 0) throw new IOException(operation + " refused: Windows error " + code);
    }
    static IntPtr Allocate(string file, bool start) {
        int size = Marshal.SizeOf<Properties>();
        IntPtr memory = Marshal.AllocHGlobal(size + 8192);
        Marshal.Copy(new byte[size + 8192], 0, memory, size + 8192);
        var p = new Properties();
        p.Wnode.Size = (uint)(size + 8192);
        p.Wnode.Flags = 0x20000;
        p.Wnode.Clock = 1; // QueryPerformanceCounter, shared with Stopwatch.
        p.LoggerNameOffset = (uint)size;
        p.EnableFlags = ProcessFlag;
        if (start) {
            p.Wnode.Guid = Guid.NewGuid();
            p.BufferSize = 256;
            p.MinimumBuffers = 64;
            p.MaximumBuffers = 256;
            p.LogFileMode = Sequential | SystemLogger;
        }
        if (file != null) {
            if (file.Length >= 2048) throw new IOException("ETL path is too long");
            p.LogFileNameOffset = (uint)(size + 2048);
            Marshal.Copy((file + "\0").ToCharArray(), 0, IntPtr.Add(memory, size + 2048), file.Length + 1);
        }
        Marshal.Copy((Name + "\0").ToCharArray(), 0, IntPtr.Add(memory, size), Name.Length + 1);
        Marshal.StructureToPtr(p, memory, false);
        return memory;
    }
    Properties Control(uint operation, string file = null) {
        IntPtr p = Allocate(file, false);
        try {
            Require(ControlTraceW(controller, Name, p, operation), "ControlTrace(" + operation + ")");
            var result = Marshal.PtrToStructure<Properties>(p);
            // llmlint: ignore[changed_behavior_has_e2e] ETW owns these returned loss counters; a caller cannot set them through a valid tracing request. Deterministically substituting a nonzero native result would require the kernel double this task forbids. Real controller death and truncated ETL regressions instead prove unavailable observation is refused and released.
            if (result.EventsLost != 0 || result.LogBuffersLost != 0 || result.RealTimeBuffersLost != 0)
                throw new IOException("incomplete ETW observation: events or buffers lost");
            return result;
        } finally { Marshal.FreeHGlobal(p); }
    }
    string PathFor(int index) { return Path.Combine(directory, "chunk-" + index + ".etl"); }

    static void Layout<T>(int size, params (string Field, int Offset)[] fields) {
        if (Marshal.SizeOf<T>() != size) throw new IOException("unsupported SDK ABI size: " + typeof(T).Name);
        foreach (var field in fields)
            if (Marshal.OffsetOf<T>(field.Field).ToInt32() != field.Offset)
                throw new IOException("unsupported SDK ABI offset: " + typeof(T).Name + "." + field.Field);
    }
    public static void CheckLayout() {
        // Offsets of the cited SDK x64/ARM64 declarations. Real controller,
        // consumer and TDH calls exercise these layouts on both target arches.
        if (IntPtr.Size != 8) throw new IOException("unsupported Windows ETW pointer width");
        Layout<Wnode>(48, ("Size",0), ("Provider",4), ("Context",8), ("Time",16), ("Guid",24), ("Clock",40), ("Flags",44));
        Layout<Properties>(120, ("Wnode",0), ("BufferSize",48), ("MinimumBuffers",52), ("MaximumBuffers",56), ("MaximumFileSize",60), ("LogFileMode",64), ("FlushTimer",68), ("EnableFlags",72), ("AgeLimit",76), ("NumberOfBuffers",80), ("FreeBuffers",84), ("EventsLost",88), ("BuffersWritten",92), ("LogBuffersLost",96), ("RealTimeBuffersLost",100), ("LoggerThread",104), ("LogFileNameOffset",112), ("LoggerNameOffset",116));
        Layout<Descriptor>(16, ("Id",0), ("Version",2), ("Channel",3), ("Level",4), ("Opcode",5), ("Task",6), ("Keyword",8));
        Layout<Header>(80, ("Size",0), ("Type",2), ("Flags",4), ("Property",6), ("Thread",8), ("Process",12), ("Time",16), ("Provider",24), ("Descriptor",40), ("ProcessorTime",56), ("Activity",64));
        Layout<Record>(112, ("Header",0), ("BufferContext",80), ("ExtendedCount",84), ("DataLength",86), ("ExtendedData",88), ("Data",96), ("Context",104));
        Layout<EventHeader>(48, ("Size",0), ("FieldType",2), ("Version",4), ("Thread",8), ("Process",12), ("Time",16), ("Guid",24), ("ProcessorTime",40));
        Layout<Event>(88, ("Header",0), ("Instance",48), ("ParentInstance",52), ("Parent",56), ("Data",72), ("Length",80), ("Context",84));
        Layout<TimeZone>(172, ("Bias",0), ("StandardName",4), ("StandardDate",68), ("StandardBias",84), ("DaylightName",88), ("DaylightDate",152), ("DaylightBias",168));
        Layout<FileHeader>(280, ("BufferSize",0), ("Version",4), ("ProviderVersion",8), ("Processors",12), ("EndTime",16), ("TimerResolution",24), ("MaximumFileSize",28), ("Mode",32), ("BuffersWritten",36), ("Instance",40), ("LoggerName",56), ("FileName",64), ("Zone",72), ("BootTime",248), ("Frequency",256), ("StartTime",264), ("Flags",272), ("BuffersLost",276));
        Layout<LogFile>(448, ("FileName",0), ("LoggerName",8), ("CurrentTime",16), ("BuffersRead",24), ("Mode",28), ("CurrentEvent",32), ("Header",120), ("BufferCallback",400), ("BufferSize",408), ("Filled",412), ("EventsLost",416), ("EventCallback",424), ("Kernel",432), ("Context",440));
        Layout<PropertyData>(16, ("Name",0), ("Index",8), ("Reserved",12));
    }
    public PrintobserverTrace(string scratch) {
        CheckLayout();
        directory = scratch;
        try {
            try { mutex.WaitOne(); } catch (AbandonedMutexException) { }
            held = true;
            // Only our reserved logger is recovered, while holding the same
            // global mutex even if the previous controller died abnormally.
            IntPtr stale = Allocate(null, false);
            try {
                uint stopped = ControlTraceW(0, Name, stale, 1);
                if (stopped != 4201) Require(stopped, "recover owned kernel logger");
            } finally { Marshal.FreeHGlobal(stale); }
            IntPtr p = Allocate(PathFor(0), true);
            try { Require(StartTraceW(out controller, Name, p), "StartTrace (administrator required)"); }
            finally { Marshal.FreeHGlobal(p); }
            Guid provider = ProcessProvider;
            Require(EnableTraceEx2(controller, ref provider, 1, 5, ulong.MaxValue, 0, 5000, IntPtr.Zero), "enable manifest process lifetimes");
            provider = RequestProvider;
            Require(EnableTraceEx2(controller, ref provider, 1, 5, ulong.MaxValue, 0, 5000, IntPtr.Zero), "enable TCP connect requests");
        } catch { Dispose(); throw; }
    }
    public string SessionId { get { return Control(0).Wnode.Guid.ToString("D"); } }
    public long Prepare() {
        endpoints.Clear(); rootSeen = false; rootEnded = false; root = 0;
        invocation = 0;
        boundary = Stopwatch.GetTimestamp();
        return boundary;
    }
    static byte[] Property(IntPtr record, string name) {
        IntPtr wide = Marshal.StringToHGlobalUni(name);
        try {
            var descriptor = new PropertyData { Name = (ulong)wide.ToInt64(), Index = uint.MaxValue };
            uint length;
            Require(TdhGetPropertySize(record, 0, IntPtr.Zero, 1, ref descriptor, out length), "TDH property size " + name);
            // llmlint: ignore[changed_behavior_has_e2e] TDH owns this native property size; valid provider events cannot choose an oversized schema field. Forcing this guard requires the forbidden kernel/tracer double. A real corrupted closed ETL exercises native refusal instead.
    if (length > 65536) throw new IOException("invalid ETW property length");
            byte[] value = new byte[length];
            Require(TdhGetProperty(record, 0, IntPtr.Zero, 1, ref descriptor, length, value), "TDH property " + name);
            return value;
        } finally { Marshal.FreeHGlobal(wide); }
    }
    // SOCKADDR_IN / SOCKADDR_IN6 stable Winsock ABI, carried by the TDH Binary
    // RemoteAddress property. SDK declarations:
    // https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/ws2def.h
    // https://raw.githubusercontent.com/microsoft/win32metadata/main/generation/WinSDK/RecompiledIdlHeaders/shared/ws2ipdef.h
    static string Socket(byte[] value) {
        // llmlint: ignore[changed_behavior_has_e2e] The TDH provider supplies this sockaddr buffer with its fixed Windows schema length; a valid TCP request cannot choose malformed bytes. Replacing the native property or callback would violate the task ban on kernel/tracer doubles. Real IPv4/IPv6 sockets and a corrupted actual ETL prove accepted evidence and native refusal.
        if (value == null || (value.Length != 16 && value.Length != 28))
            throw new IOException("invalid TCP request socket payload length");
        ushort family = BitConverter.ToUInt16(value, 0);
        ushort port = (ushort)((value[2] << 8) | value[3]);
        if (family == 2 && value.Length == 16)
            return new IPEndPoint(new IPAddress(value.Skip(4).Take(4).ToArray()), port).ToString();
        if (family == 23 && value.Length == 28)
            return new IPEndPoint(new IPAddress(value.Skip(8).Take(16).ToArray(), BitConverter.ToUInt32(value, 24)), port).ToString();
        // llmlint: ignore[changed_behavior_has_e2e] The TCP request provider emits AF_INET or AF_INET6 with its defined sockaddr size. No valid socket request emits a third family or incompatible size here; a synthetic callback would be the forbidden kernel double. Both valid families are exercised against real accepted/refused sockets.
        throw new IOException("unknown TCP request socket family or length");
    }
    // Field schemas are the enabled providers' own manifests, queried by TDH
    // for each native event. No classic payload offsets or version copies.
    static uint ProcessId(IntPtr record, string field) {
        byte[] value = Property(record, field);
        // llmlint: ignore[changed_behavior_has_e2e] The explicitly enabled process provider owns this DWORD property schema. A valid process cannot emit a differently sized process ID; substituting its native property would be the forbidden tracer double. Real starts, descendants and exits exercise the valid provider boundary.
    if (value.Length != sizeof(uint)) throw new IOException("invalid manifest process ID property");
        return BitConverter.ToUInt32(value, 0);
    }
    static Observation Decode(IntPtr pointer) {
        var record = Marshal.PtrToStructure<Record>(pointer);
        var h = record.Header;
        if (h.Provider == ProcessProvider && (h.Descriptor.Id == 1 || h.Descriptor.Id == 2))
            return new Observation { Time = h.Time, Kind = h.Descriptor.Id,
                Pid = ProcessId(pointer, "ProcessID"),
                Parent = h.Descriptor.Id == 1 ? ProcessId(pointer, "ParentProcessID") : 0 };
        if (h.Provider == RequestProvider && h.Descriptor.Id == 1002)
            return new Observation { Time = h.Time, Kind = 3, Pid = h.Process,
                Address = Socket(Property(pointer, "RemoteAddress")) };
        return null;
    }
    void Consume(Observation eventData) {
        if (eventData.Kind == 1) {
            // llmlint: ignore[changed_behavior_has_e2e] Complete kernel start/end events cannot overlap two lifetimes of one live PID. This defense detects corrupted or incomplete OS evidence, which no valid process launch can manufacture without the forbidden tracer/kernel double; real descendants and adjacent invocations exercise valid lifetime/ancestry attribution.
            if (alive.ContainsKey(eventData.Pid)) throw new IOException("process lifetime overlapped: incomplete ETW process events");
            Lifetime parent;
            long identity = ++sequence;
            long ancestor = 0;
            if (eventData.Pid == root && eventData.Time >= boundary && !rootSeen) {
                rootSeen = true;
                invocation = identity;
                ancestor = identity;
            } else if (alive.TryGetValue(eventData.Parent, out parent)) {
                ancestor = parent.Root;
            }
            alive.Add(eventData.Pid, new Lifetime { Identity = identity, Root = ancestor });
        } else if (eventData.Kind == 2) {
            Lifetime lifetime;
            if (alive.TryGetValue(eventData.Pid, out lifetime)) {
                if (lifetime.Identity == invocation) rootEnded = true;
                alive.Remove(eventData.Pid);
            }
        } else {
            Lifetime lifetime;
            if (alive.TryGetValue(eventData.Pid, out lifetime) && invocation != 0 && lifetime.Root == invocation) endpoints.Add(eventData.Address);
        }
    }
    void Read(string path) {
        var records = new List<Observation>();
        decodingError = null;
        EventCallback callback = pointer => {
            try { var item = Decode(pointer); if (item != null) records.Add(item); }
            catch (Exception error) { decodingError = error; }
        };
        BufferCallback buffers = pointer => {
            var state = Marshal.PtrToStructure<LogFile>(pointer);
            // llmlint: ignore[changed_behavior_has_e2e] These counters are kernel-owned output delivered by ProcessTrace. The real-kernel regression must not substitute a callback or fabricate its native structure to force loss; it exercises a destroyed completed ETL file and requires native refusal instead.
            if (state.EventsLost != 0 || state.Header.BuffersLost != 0)
                decodingError = new IOException("incomplete ETL: events or buffers lost");
            return 1;
        };
        IntPtr name = Marshal.StringToHGlobalUni(path);
        var file = new LogFile { FileName = name, Mode = 0x10000000 | 0x1000,
            EventCallback = Marshal.GetFunctionPointerForDelegate(callback), BufferCallback = Marshal.GetFunctionPointerForDelegate(buffers) };
        ulong reader = ulong.MaxValue;
        try {
            reader = OpenTraceW(ref file);
            if (reader == ulong.MaxValue) throw new IOException("OpenTrace refused completed chunk");
            if (file.Header.EndTime == 0) throw new IOException("ETL chunk was not closed by rotation");
            Require(ProcessTrace(new ulong[] { reader }, 1, IntPtr.Zero, IntPtr.Zero), "ProcessTrace closed chunk");
            if (decodingError != null) throw new IOException("ETW decode incomplete", decodingError);
            foreach (var item in records.OrderBy(item => item.Time)) Consume(item);
        } finally {
            if (reader != ulong.MaxValue) Require(CloseTrace(reader), "CloseTrace");
            Marshal.FreeHGlobal(name);
            GC.KeepAlive(callback); GC.KeepAlive(buffers);
        }
        File.Delete(path);
    }
    // Regression-only fault input: destroy the actual completed ETL file,
    // exercising native OpenTrace refusal rather than substituting a decoder.
    public void InvalidateNextChunk() { invalidateNext = true; }
    // Snapshot the actual flushed, active log before rotation. Its native
    // header remains unclosed even though the copied file itself is finite.
    public void ReadUnclosedChunk() {
        Control(3);
        string snapshot = Path.Combine(directory, "unclosed.etl");
        File.Copy(PathFor(chunk), snapshot);
        Read(snapshot);
    }
    public string[] Finish(uint process) {
        root = process;
        // A root's end is not the end of its descendants. Continue rotating
        // until every observed member's lifetime has ended. The deadline only
        // refuses runaway descendants; it never turns waiting into evidence.
        var deadline = Stopwatch.StartNew();
        do {
            Control(3); // Flush active buffers synchronously.
            string closed = PathFor(chunk);
            Control(2, PathFor(++chunk)); // Switch files without stopping ETW.
            if (invalidateNext) {
                invalidateNext = false;
                File.WriteAllBytes(closed, new byte[] { 0 });
            }
            Read(closed); // ProcessTrace returns at the end of the closed file.
            Control(0); // Refuse loss while the completed chunk was decoded.
            if (deadline.Elapsed > TimeSpan.FromSeconds(30)) throw new IOException("incomplete process tree: descendants did not exit");
        } while (!rootSeen || !rootEnded || alive.Values.Any(item => item.Root == invocation));
        return endpoints.OrderBy(item => item).ToArray();
    }
    public void Dispose() {
        if (disposed) return;
        disposed = true;
        try {
            if (controller != 0) {
                IntPtr p = Allocate(null, false);
                try { Require(ControlTraceW(controller, Name, p, 1), "stop owned kernel logger"); }
                finally { Marshal.FreeHGlobal(p); controller = 0; }
            }
        } finally {
            if (held) { mutex.ReleaseMutex(); held = false; }
            mutex.Dispose();
        }
    }
}
