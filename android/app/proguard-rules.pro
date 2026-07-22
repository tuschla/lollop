# R8 keep rules for the release (minified) build.

# --- JNI bridge -------------------------------------------------------------
# libshared.so resolves callbacks by their fully-qualified JNI symbol name
# (Java_app_milestone_Core_update). Renaming or removing the Core
# class or its native methods breaks symbol resolution → UnsatisfiedLinkError.
-keep class app.milestone.Core {
    native <methods>;
}
-keepclasseswithmembernames,includedescriptorclasses class * {
    native <methods>;
}

# --- kotlinx.serialization --------------------------------------------------
# The core ViewModel and its nested types are decoded via generated
# `$$serializer` classes and a synthetic `serializer()` method; R8 must keep
# both plus the annotations the serializer reflects on.
-keepattributes RuntimeVisibleAnnotations,AnnotationDefault,InnerClasses
-keepclassmembers class app.milestone.** {
    *** Companion;
}
-keepclasseswithmembers class app.milestone.** {
    kotlinx.serialization.KSerializer serializer(...);
}
-keep,includedescriptorclasses class app.milestone.**$$serializer {
    *;
}

# --- osmdroid ---------------------------------------------------------------
# Tile/overlay classes are loaded reflectively from XML/config.
-keep class org.osmdroid.** { *; }
-dontwarn org.osmdroid.**
