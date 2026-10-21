# Screen-recording helpers for the live demo (dot-source this file: . tools\screenrec.ps1).
# Starts the app, maximizes it, and captures the window at ~10 fps into JPEG files while a script drives it
# with real mouse / key events. The mouse cursor is not part of a screen copy, so every frame's cursor
# position is logged to frames.csv (t_ms, x, y, down) and drawn later by tools/gen_demo_live.py.
# NOTE: the real mouse and keyboard are used while recording. Do not touch them until it ends.
Add-Type -ReferencedAssemblies System.Drawing, System.Windows.Forms -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class Rec {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct PT { public int X, Y; }
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out PT p);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, int dx, int dy, int d, UIntPtr e);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
    [DllImport("user32.dll")] static extern short VkKeyScan(char c);
    [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref PT p);

    public static Process App;
    static IntPtr hwnd;
    static Thread cap;
    static volatile bool running;
    static volatile int down;
    static Stopwatch clock = new Stopwatch();
    static StringBuilder marks = new StringBuilder();
    static string markDir;
    // caption change: from now on, show this text (written to marks.csv as "t_ms<TAB>text")
    public static void Mark(string text) { lock (marks) { marks.AppendFormat("{0}\t{1}\n", clock.ElapsedMilliseconds, text); } }

    public static void Init() { SetProcessDPIAware(); }

    public static void Launch(string exe, string args) {
        App = Process.Start(new ProcessStartInfo(exe, args) { UseShellExecute = false });
        for (int i = 0; i < 200 && (App.MainWindowHandle == IntPtr.Zero); i++) { Thread.Sleep(100); App.Refresh(); }
        hwnd = App.MainWindowHandle;
        ShowWindow(hwnd, 3); // maximize
        Thread.Sleep(500);
        SetForegroundWindow(hwnd);
        Thread.Sleep(2500); // first frames, font loading
    }

    // client area of the app window in screen pixels
    public static int[] Client() {
        RECT r; GetClientRect(hwnd, out r);
        PT p = new PT(); ClientToScreen(hwnd, ref p);
        return new int[] { p.X, p.Y, r.R - r.L, r.B - r.T };
    }

    public static void StartCapture(string dir, int fps) {
        Directory.CreateDirectory(dir);
        foreach (var f in Directory.GetFiles(dir)) File.Delete(f);
        int[] c = Client(); markDir = dir; marks.Clear();
        var log = new StringBuilder("n,t_ms,x,y,down\n");
        var enc = ImageCodecInfo.GetImageEncoders();
        ImageCodecInfo jpg = null; foreach (var e in enc) if (e.MimeType == "image/jpeg") jpg = e;
        var ep = new EncoderParameters(1); ep.Param[0] = new EncoderParameter(System.Drawing.Imaging.Encoder.Quality, 88L);
        running = true; clock.Restart();
        cap = new Thread(() => {
            int n = 0; int step = 1000 / fps;
            using (var bmp = new Bitmap(c[2], c[3], PixelFormat.Format24bppRgb))
            using (var g = Graphics.FromImage(bmp)) {
                while (running) {
                    long t0 = clock.ElapsedMilliseconds;
                    g.CopyFromScreen(c[0], c[1], 0, 0, new Size(c[2], c[3]));
                    PT p; GetCursorPos(out p);
                    bmp.Save(Path.Combine(dir, n.ToString("D5") + ".jpg"), jpg, ep);
                    log.AppendFormat("{0},{1},{2},{3},{4}\n", n, t0, p.X - c[0], p.Y - c[1], down);
                    n++;
                    long dt = clock.ElapsedMilliseconds - t0;
                    if (dt < step) Thread.Sleep((int)(step - dt));
                }
            }
            File.WriteAllText(Path.Combine(dir, "frames.csv"), log.ToString());
        });
        cap.Start();
    }

    public static void StopCapture() {
        running = false; if (cap != null) cap.Join();
        lock (marks) { File.WriteAllText(Path.Combine(markDir, "marks.csv"), marks.ToString(), new UTF8Encoding(false)); }
    }

    // window-client coordinates -> screen
    static void To(int x, int y) { int[] c = Client(); SetCursorPos(c[0] + x, c[1] + y); }

    public static void MoveTo(int x, int y, int ms) {
        int[] c = Client(); PT p; GetCursorPos(out p);
        double sx = p.X, sy = p.Y, ex = c[0] + x, ey = c[1] + y;
        int steps = Math.Max(1, ms / 15);
        for (int i = 1; i <= steps; i++) {
            double t = (double)i / steps; t = t * t * (3 - 2 * t); // ease in-out
            SetCursorPos((int)(sx + (ex - sx) * t), (int)(sy + (ey - sy) * t));
            Thread.Sleep(15);
        }
    }

    public static void Click(int x, int y) {
        MoveTo(x, y, 350); Thread.Sleep(120);
        down = 1; mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(90);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); down = 0; Thread.Sleep(150);
    }
    public static void RightClick(int x, int y) {
        MoveTo(x, y, 350); Thread.Sleep(120);
        mouse_event(0x0008, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(90);
        mouse_event(0x0010, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(150);
    }
    public static void DoubleClick(int x, int y) {
        MoveTo(x, y, 350); Thread.Sleep(120);
        for (int i = 0; i < 2; i++) { mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(40); mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(60); }
        Thread.Sleep(150);
    }
    // press at (x1,y1), move slowly to (x2,y2), release
    public static void Drag(int x1, int y1, int x2, int y2, int ms) {
        MoveTo(x1, y1, 300); Thread.Sleep(100);
        down = 1; mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(80);
        MoveTo(x2, y2, ms);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); down = 0; Thread.Sleep(150);
    }
    public static void Wheel(int x, int y, int notches) {
        MoveTo(x, y, 250);
        for (int i = 0; i < Math.Abs(notches); i++) { mouse_event(0x0800, 0, 0, notches > 0 ? 120 : -120, UIntPtr.Zero); Thread.Sleep(60); }
    }

    public static void Key(byte vk) {
        byte sc = (byte)MapVirtualKey(vk, 0); // winit identifies keys by scan code
        keybd_event(vk, sc, 0, UIntPtr.Zero); Thread.Sleep(40); keybd_event(vk, sc, 2, UIntPtr.Zero); Thread.Sleep(60);
    }
    // modifier held while a key is pressed (Ctrl = 0x11), e.g. Chord(0x11, 0x41) is Ctrl+A
    public static void Chord(byte mod, byte vk) {
        keybd_event(mod, (byte)MapVirtualKey(mod, 0), 0, UIntPtr.Zero); Thread.Sleep(40);
        Key(vk);
        keybd_event(mod, (byte)MapVirtualKey(mod, 0), 2, UIntPtr.Zero); Thread.Sleep(60);
    }
    // ASCII only (the shift state is handled); delay between keys so the typing is visible
    public static void Type(string s, int delayMs) {
        foreach (char ch in s) {
            short v = VkKeyScan(ch); byte vk = (byte)(v & 0xff); bool shift = (v & 0x100) != 0;
            if (shift) keybd_event(0x10, 0x2A, 0, UIntPtr.Zero);
            Key(vk);
            if (shift) keybd_event(0x10, 0x2A, 2, UIntPtr.Zero);
            Thread.Sleep(delayMs);
        }
    }
    public static void Kill() { try { if (App != null && !App.HasExited) App.Kill(); } catch { } }
}
'@

[Rec]::Init()
function Rec-Launch($exe, $argLine) { [Rec]::Launch($exe, $argLine) }
function Rec-Client { [Rec]::Client() }
function Rec-Start($dir, $fps = 10) { [Rec]::StartCapture($dir, $fps) }
function Rec-Stop { [Rec]::StopCapture() }
function Click($x, $y) { [Rec]::Click($x, $y) }
function RightClick($x, $y) { [Rec]::RightClick($x, $y) }
function DoubleClick($x, $y) { [Rec]::DoubleClick($x, $y) }
function Drag($x1, $y1, $x2, $y2, $ms = 900) { [Rec]::Drag($x1, $y1, $x2, $y2, $ms) }
function Wheel($x, $y, $n) { [Rec]::Wheel($x, $y, $n) }
function MouseTo($x, $y, $ms = 500) { [Rec]::MoveTo($x, $y, $ms) }
function Typing($s, $delay = 110) { [Rec]::Type($s, $delay) }
function PressKey($vk) { [Rec]::Key([byte]$vk) }   # e.g. 0x0D Enter, 0x1B Esc, 0x41 A
function CtrlKey($vk) { [Rec]::Chord(0x11, [byte]$vk) }
function Mark($text) { [Rec]::Mark($text) }
function Pause($ms) { Start-Sleep -Milliseconds $ms }
