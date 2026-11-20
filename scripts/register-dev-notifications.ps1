# Registers a Start Menu shortcut for the FrameForge dev build, carrying the
# AppUserModelID (AUMID) Windows requires to deliver toast notifications.
#
# Windows resolves a toast's identity (name, icon, Action Center entry)
# through a Start Menu shortcut whose System.AppUserModel.ID property
# matches the AUMID the app registers its toast under. An installer creates
# that shortcut automatically; `pnpm tauri dev` launches the raw exe with no
# shortcut at all, so Windows can't resolve an identity for it and silently
# drops every toast before it reaches Action Center.
#
# Run this once (idempotent - safe to re-run). After that, dev-mode toast
# notifications work exactly like an installed build's. Re-run only if the
# "identifier" in src-tauri/tauri.conf.json ever changes.

$ErrorActionPreference = "Stop"

$AppId = "com.wyrmstudios.frameforge"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$ExePath = Join-Path $RepoRoot "src-tauri\target\debug\warframe-companion.exe"

if (-not (Test-Path $ExePath)) {
    Write-Error "Dev exe not found at $ExePath`nBuild it once with 'pnpm tauri dev' (or the desktop .bat), then re-run this script."
    exit 1
}

$StartMenu = [Environment]::GetFolderPath("Programs")
$ShortcutPath = Join-Path $StartMenu "FrameForge (Dev).lnk"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

public static class DevAumidShortcut
{
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    internal class CShellLink { }

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("000214F9-0000-0000-C000-000000000046")]
    internal interface IShellLinkW
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszFile, int cchMaxPath, IntPtr pfd, uint fFlags);
        void GetIDList(out IntPtr ppidl);
        void SetIDList(IntPtr pidl);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszName, int cchMaxName);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string pszName);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszDir, int cchMaxPath);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string pszDir);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszArgs, int cchMaxPath);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string pszArgs);
        void GetHotkey(out short pwHotkey);
        void SetHotkey(short wHotkey);
        void GetShowCmd(out int piShowCmd);
        void SetShowCmd(int iShowCmd);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder pszIconPath, int cchIconPath, out int piIcon);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string pszIconPath, int iIcon);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string pszPathRel, uint dwReserved);
        void Resolve(IntPtr hwnd, uint fFlags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string pszFile);
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct PROPERTYKEY
    {
        public Guid fmtid;
        public int pid;
        public PROPERTYKEY(Guid fmtid, int pid) { this.fmtid = fmtid; this.pid = pid; }
    }

    [StructLayout(LayoutKind.Explicit)]
    internal struct PROPVARIANT
    {
        [FieldOffset(0)] public ushort vt;
        [FieldOffset(8)] public IntPtr pwszVal;
    }

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99")]
    internal interface IPropertyStore
    {
        void GetCount(out uint cProps);
        void GetAt(uint iProp, out PROPERTYKEY pkey);
        void GetValue(ref PROPERTYKEY key, out PROPVARIANT pv);
        void SetValue(ref PROPERTYKEY key, ref PROPVARIANT pv);
        void Commit();
    }

    [DllImport("ole32.dll")]
    internal static extern int PropVariantClear(ref PROPVARIANT pvar);

    public static void Create(string shortcutPath, string targetPath, string workingDir, string appId)
    {
        var link = (IShellLinkW)new CShellLink();
        link.SetPath(targetPath);
        link.SetWorkingDirectory(workingDir);
        link.SetDescription("FrameForge (dev build) - registered only so Windows can deliver toast notifications in dev mode.");

        var pkeyAppId = new PROPERTYKEY(new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), 5); // PKEY_AppUserModel_ID
        var pv = new PROPVARIANT();
        pv.vt = 31; // VT_LPWSTR
        pv.pwszVal = Marshal.StringToCoTaskMemUni(appId);

        var propStore = (IPropertyStore)link;
        propStore.SetValue(ref pkeyAppId, ref pv);
        propStore.Commit();
        PropVariantClear(ref pv);

        ((IPersistFile)link).Save(shortcutPath, true);
    }
}
"@

[DevAumidShortcut]::Create($ShortcutPath, $ExePath, (Split-Path $ExePath), $AppId)

Write-Host "Registered: $ShortcutPath"
Write-Host "AUMID:      $AppId"
Write-Host "Target:     $ExePath"
Write-Host ""
Write-Host "Keep launching FrameForge from the desktop .bat as usual - this shortcut"
Write-Host "exists only so Windows can resolve the dev process notification identity."
