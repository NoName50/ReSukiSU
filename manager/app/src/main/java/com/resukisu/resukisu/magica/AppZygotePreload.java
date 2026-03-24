package com.resukisu.resukisu.magica;

import android.annotation.SuppressLint;
import android.app.ZygotePreload;
import android.content.pm.ApplicationInfo;
import android.util.Log;

import androidx.annotation.NonNull;

import java.io.File;

@SuppressLint("NewApi")
public class AppZygotePreload implements ZygotePreload {
    public static final String TAG = "KernelSUMagica";

    private static native void forkDontCareAndExecKsud(String ksudPath, boolean isColorOS);

    @Override
    public void doPreload(@NonNull ApplicationInfo appInfo) {
        File f = new File(appInfo.nativeLibraryDir, "libksud.so");
        try {
            System.loadLibrary("kernelsu");
            boolean isColorOS = false;
            try {
                isColorOS = (int) (Class.forName("com.oplus.os.OplusBuild$VERSION").getField("SDK_VERSION").get(null)) != 0;
                Log.d(TAG, "isColorOS: " + isColorOS);
            } catch (Exception ignored) { }
            Log.d(TAG, "executing magica ...");
            forkDontCareAndExecKsud(f.getAbsolutePath(), isColorOS);
        } catch (Throwable t) {
            Log.e(TAG, "failed to late load", t);
        }
    }
}
