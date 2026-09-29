# JNA reads native structures and callbacks by reflection, and the UniFFI bindings are called through it.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class io.github.josevini.clipsync.core.** { *; }
-dontwarn java.awt.**
