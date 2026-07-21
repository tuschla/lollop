# R8 keep rules for the release (minified) build.

# --- JNI bridge -------------------------------------------------------------
# libshared.so resolves callbacks by their fully-qualified JNI symbol name
# (Java_de_tuschla_fitnessanlage_Core_update). Renaming or removing the Core
# class or its native methods breaks symbol resolution → UnsatisfiedLinkError.
-keep class de.tuschla.fitnessanlage.Core {
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
-keepclassmembers class de.tuschla.fitnessanlage.** {
    *** Companion;
}
-keepclasseswithmembers class de.tuschla.fitnessanlage.** {
    kotlinx.serialization.KSerializer serializer(...);
}
-keep,includedescriptorclasses class de.tuschla.fitnessanlage.**$$serializer {
    *;
}

# --- osmdroid ---------------------------------------------------------------
# Tile/overlay classes are loaded reflectively from XML/config.
-keep class org.osmdroid.** { *; }
-dontwarn org.osmdroid.**
