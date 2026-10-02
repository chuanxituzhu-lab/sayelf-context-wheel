using System;
using System.Runtime.InteropServices;
public static class NativeTest {
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls,string name);
 public static IntPtr FindTitle(string title){return FindWindow(null,title);}
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,System.Text.StringBuilder text,int n);
 public static string TitlesForProcess(int process){string result="";EnumWindows((h,l)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==(uint)process){var title=new System.Text.StringBuilder(256);GetWindowText(h,title,256);result+=h+":"+title+";";}return true;},IntPtr.Zero);return result;}
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc fn,IntPtr l);
 delegate bool EnumProc(IntPtr h,IntPtr l);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a,uint b,bool attach);
 [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
 [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
 [DllImport("user32.dll")] static extern IntPtr SetFocus(IntPtr h);
 public static IntPtr ForProcess(int process){IntPtr result=IntPtr.Zero;EnumWindows((h,l)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==(uint)process){var title=new System.Text.StringBuilder(256);GetWindowText(h,title,256);if(title.ToString()=="CWE isolated test fixture"){result=h;return false;}}return true;},IntPtr.Zero);return result;}
 public static bool Focus(IntPtr h){uint pid;uint foreground=GetWindowThreadProcessId(GetForegroundWindow(),out pid);uint target=GetWindowThreadProcessId(h,out pid);uint mine=GetCurrentThreadId();bool a=AttachThreadInput(mine,foreground,true);bool b=target!=foreground && AttachThreadInput(mine,target,true);BringWindowToTop(h);bool ok=SetForegroundWindow(h);SetFocus(h);if(b)AttachThreadInput(mine,target,false);if(a)AttachThreadInput(mine,foreground,false);return ok;}
 [DllImport("user32.dll")] static extern uint SendInput(uint n,Input[] inputs,int cb);
 [DllImport("user32.dll")] static extern int GetSystemMetrics(int n);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] struct Mouse {public int dx,dy;public uint data,flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=32)] struct Union {[FieldOffset(0)]public Mouse mouse;}
 [StructLayout(LayoutKind.Sequential)] struct Input {public uint type; public Union u;}
 public static void Move(int x,int y){Input v=new Input();v.u.mouse.dx=(int)((long)x*65535/(GetSystemMetrics(0)-1));v.u.mouse.dy=(int)((long)y*65535/(GetSystemMetrics(1)-1));v.u.mouse.flags=0x8001;v.u.mouse.extra=new UIntPtr(0xC0E123);if(SendInput(1,new[]{v},Marshal.SizeOf(typeof(Input)))!=1)throw new Exception("Tagged move failed");}
 public static void FastMark(int button){Move(600,400);Button(true,button);System.Threading.Thread.Sleep(15);Move(900,400);System.Threading.Thread.Sleep(15);Button(false,button);}
 public static bool EarlyVisible(IntPtr overlay,int button){Move(600,400);Button(true,button);System.Threading.Thread.Sleep(25);return IsWindowVisible(overlay);}
 public static void Button(bool down,int button){Input v=new Input();v.u.mouse.flags=button==3?(down?0x20u:0x40u):(down?0x80u:0x100u);v.u.mouse.data=button==3?0u:(uint)button;v.u.mouse.extra=new UIntPtr(0xC0E123);if(SendInput(1,new[]{v},Marshal.SizeOf(typeof(Input)))!=1)throw new Exception("SendInput failed");}
}
