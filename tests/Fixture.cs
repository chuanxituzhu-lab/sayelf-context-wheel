using System;
using System.IO;
using System.Windows.Forms;
public class Fixture : Form {
 readonly string log;
 public Fixture(string path) {log=path;Text="CWE isolated test fixture";Width=640;Height=400;KeyPreview=true;ImeMode=ImeMode.Disable;KeyDown+=(s,e)=>{File.AppendAllText(log,"key:"+e.KeyCode+"\n");if(e.Control)e.SuppressKeyPress=true;};KeyPress+=(s,e)=>File.AppendAllText(log,"char:"+((int)e.KeyChar)+"\n");var box=new TextBox{ImeMode=ImeMode.Disable,Multiline=true,Dock=DockStyle.Fill,Text="Context Wheel 本地测试窗口，不含用户文档"};Controls.Add(box);Activated+=(s,e)=>box.Focus();}
 [STAThread] public static void Main(string[] args){Application.Run(new Fixture(args[0]));}
}
