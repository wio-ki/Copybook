using System;
using System.Runtime.InteropServices;

public static class CopybookSmokeWindow
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr CreateWindowExW(uint exStyle, string className, string title,
        uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu,
        IntPtr instance, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] private static extern bool BringWindowToTop(IntPtr window);
    [DllImport("user32.dll")] private static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowTextW(IntPtr window, System.Text.StringBuilder text, int size);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    private delegate bool EnumProc(IntPtr window, IntPtr parameter);

    public static bool Focus(IntPtr window)
    {
        SetForegroundWindow(window);
        uint current = GetCurrentThreadId();
        uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), out uint _);
        bool attached = current != foreground && AttachThreadInput(current, foreground, true);
        BringWindowToTop(window);
        SetForegroundWindow(window);
        if (attached) AttachThreadInput(current, foreground, false);
        if (GetForegroundWindow() != window) {
            keybd_event(0x87, 0, 0, UIntPtr.Zero); // balanced F24, no modifiers
            keybd_event(0x87, 0, 2, UIntPtr.Zero);
            SetForegroundWindow(window);
        }
        for (int attempt = 0; attempt < 30; attempt++) {
            if (GetForegroundWindow() == window) return true;
            System.Threading.Thread.Sleep(10);
        }
        return false;
    }

    public static int VisibleWindows(uint process)
    {
        int count = 0;
        EnumWindows((window, _) => {
            GetWindowThreadProcessId(window, out uint owner);
            var title=new System.Text.StringBuilder(256);
            GetWindowTextW(window,title,256);
            // Count application windows; WebView2 also owns native IME helper windows.
            if (owner == process && IsWindowVisible(window) && (title.ToString()=="Copybook" || title.ToString()=="Copybook — Settings")) count++;
            return true;
        }, IntPtr.Zero);
        return count;
    }
}
