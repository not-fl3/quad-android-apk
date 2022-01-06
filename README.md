A fork of an old `android-rs-glue` crate, compatible with building miniquad-based projects.

# Supported `[package.metadata.android]` entries

```toml
# The target Android API level.
# "android_version" is the compile SDK version. It defaults to 29.
# (target_sdk_version defaults to the value of "android_version")
# (min_sdk_version defaults to 18) It defaults to 18 because this is the minimum supported by rustc.
android_version = 29
target_sdk_version = 29
min_sdk_version = 26

# Specifies the array of targets to build for.
# Defaults to "armv7-linux-androideabi", "aarch64-linux-android", "i686-linux-android".
build_targets = [ "armv7-linux-androideabi", "aarch64-linux-android", "i686-linux-android", "x86_64-linux-android" ]

# The following values can be customized on a per bin/example basis. See multiple_targets example
# If a value is not specified for a secondary target, it will inherit the value defined in the `package.metadata.android`
# section unless otherwise noted.
#

# The Java package name for your application.
# Hyphens are converted to underscores.
# Defaults to rust.<target_name> for binaries. 
# Defaults to rust.<package_name>.example.<target_name> for examples.
# For example: for a binary "my_app", the default package name will be "rust.my_app"
# Secondary targets will not inherit the value defined in the root android configuration.
package_name = "rust.cargo.apk.advanced"

# The user-friendly name for your app, as displayed in the applications menu.
# Defaults to the target name
# Secondary targets will not inherit the value defined in the root android configuration.
label = "My Android App"

# Internal version number used to determine whether one version is more recent than another. Must be an integer.
# Defaults to 1
# See https://developer.android.com/guide/topics/manifest/manifest-element
version_code = 2

# The version number shown to users.
# Defaults to the cargo package version number
# See https://developer.android.com/guide/topics/manifest/manifest-element
version_name = "2.0"

# Path to your application's resources folder.
# If not specified, resources will not be included in the APK
res = "path/to/res_folder"

# Virtual path your application's icon for any mipmap level.
# If not specified, an icon will not be included in the APK.
icon = "@mipmap/ic_launcher"

# Path to the folder containing your application's assets.
# If not specified, assets will not be included in the APK
assets = "path/to/assets_folder"

# If set to true, makes the app run in full-screen, by adding the following line
# as an XML attribute to the manifest's <application> tag :
#     android:theme="@android:style/Theme.DeviceDefault.NoActionBar.Fullscreen
# Defaults to false.
fullscreen = false

# The maximum supported OpenGL ES version , as claimed by the manifest.
# Defaults to 2.0.
# See https://developer.android.com/guide/topics/graphics/opengl.html#manifest
opengles_version_major = 3
opengles_version_minor = 2

# They keystore you want to sign your apks with. This has to be a path relative to this Cargo.toml.
# If this is omitted, a new keystore is generated at $HOME/.android/debug.keystore and this will be used.
keystore_path = "..."
keystore_password = "..."

# Adds extra arbitrary XML attributes to the <application> tag in the manifest.
# See https://developer.android.com/guide/topics/manifest/application-element.html
[package.metadata.android.application_attributes]
"android:debuggable" = "true"
"android:hardwareAccelerated" = "true"

# Adds extra arbitrary XML attributes to the <activity> tag in the manifest.
# See https://developer.android.com/guide/topics/manifest/activity-element.html
[package.metadata.android.activity_attributes]
"android:screenOrientation" = "unspecified"
"android:uiOptions" = "none"

# Adds a uses-feature element to the manifest
# Supported keys: name, required, version
# The glEsVersion attribute is not supported using this section. 
# It can be specified using the opengles_version_major and opengles_version_minor values
# See https://developer.android.com/guide/topics/manifest/uses-feature-element
[[package.metadata.android.feature]]
name = "android.hardware.camera"

[[package.metadata.android.feature]]
name = "android.hardware.vulkan.level"
version = "1"
required = false

# Adds a uses-permission element to the manifest.
# Note that android_version 23 and higher, Android requires the application to request permissions at runtime.
# There is currently no way to do this using a pure NDK based application.
# See https://developer.android.com/guide/topics/manifest/uses-permission-element
[[package.metadata.android.permission]]
name = "android.permission.WRITE_EXTERNAL_STORAGE"
max_sdk_version = 18

[[package.metadata.android.permission]]
name = "android.permission.CAMERA"
```

# Environment Variables
Cargo-apk sets environment variables which are used to expose the appropriate C and C++ build tools to build scripts. The primary intent is to support building crates which have build scripts which use the `cc` and `cmake` crates. 

- CC : path to NDK provided `clang` wrapper for the appropriate target and android platform. 
- CXX : path to NDK provided `clang++` wrapper for the appropriate target and android platform. 
- AR : path to NDK provided `ar`
- CXXSTDLIB : `c++` to use the full featured C++ standard library provided by the NDK.
- CMAKE_TOOLCHAIN_FILE : the path to the generated CMake toolchain. This toolchain sets the ABI, overrides any target specified, and includes the toolchain provided by the NDK.
- CMAKE_GENERATOR : `Unix Makefiles` to default to `Unix Makefiles` as opposed to using the CMake default which may not be appropriate depending on platform.
- CMAKE_MAKE_PROGRAM: Path to NDK provided make.

# C++ Standard Library Compatibility Issues
When a crate links to the C++ standard library, the shared library version provided by the NDK is used. Unfortunately, dependency loading issues will cause the application to crash on older versions of android.  Once `lld` linker issues are resolved on all platforms, cargo apk will be updated to link to the static C++ library. This should resolve the compatibility issues.
