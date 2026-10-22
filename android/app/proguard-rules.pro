# R8 keep rules for the release (minified) build.

# --- JNI bridge -------------------------------------------------------------
# libshared.so exports JNI functions bound by their fully-qualified symbol name
# (Java_app_milestone_Core_update). Renaming or removing the Core class or its
# native methods breaks that binding → UnsatisfiedLinkError. The core makes no
# native→Kotlin calls, so nothing else needs keeping for JNI. (The default R8
# config already keeps native method names; this also pins the class name.)
-keep class app.milestone.Core {
    native <methods>;
}

# --- kotlinx.serialization --------------------------------------------------
# The core ViewModel and its nested types are decoded via generated
# `$$serializer` classes and a synthetic `serializer()` method. kotlinx-
# serialization (1.7+) ships consumer rules that keep the companions of
# @Serializable classes; these rules pin the serializers of this package.
-keepattributes RuntimeVisibleAnnotations,AnnotationDefault,InnerClasses
-keepclasseswithmembers class app.milestone.** {
    kotlinx.serialization.KSerializer serializer(...);
}
-keep,includedescriptorclasses class app.milestone.**$$serializer {
    *;
}

# --- osmdroid ---------------------------------------------------------------
# The tile provider and config packages are loaded reflectively from XML/config.
-keep class org.osmdroid.tileprovider.** { *; }
-keep class org.osmdroid.config.** { *; }
-dontwarn org.osmdroid.**
