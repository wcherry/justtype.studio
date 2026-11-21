# kotlinx.serialization: keep the generated serializers for the DTOs core decodes.
-keepattributes *Annotation*, InnerClasses
-keepclassmembers class studio.justtype.** {
    *** Companion;
}
-keepclasseswithmembers class studio.justtype.** {
    kotlinx.serialization.KSerializer serializer(...);
}
# The editor page calls into this by name.
-keepclassmembers class studio.justtype.app.editor.EditorBridge {
    @android.webkit.JavascriptInterface <methods>;
}
-dontwarn okhttp3.internal.platform.**
-dontwarn org.conscrypt.**
-dontwarn org.bouncycastle.**
-dontwarn org.openjsse.**
