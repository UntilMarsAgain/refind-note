import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 37
    namespace = "com.untilmarsagain.refindnote"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.untilmarsagain.refindnote"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
          // **仅供本机自测**：密钥与口令全在 `local.properties` 里，
          // 而那个文件在 `.gitignore` 里 —— 所以别人克隆这个工程不会被
          // 卷进我的签名里，真要发布得另配一份正式密钥。
          create("debugSign") {
              val storePath = System.getenv("REFIND_DEBUG_KEYSTORE")
                  ?: (project.findProperty("refindDebugKeystore") as String?)
              if (storePath != null && file(storePath).exists()) {
                  storeFile = file(storePath)
                  storePassword = "android"
                  keyAlias = "androiddebugkey"
                  keyPassword = "android"
              }
          }
      }

      buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
            // 有密钥才签；没有就留空 —— 那样产出的 APK 是 unsigned，
            // 装不上真机。明确失败比装到一半才报错好。
            val debugSign = signingConfigs.findByName("debugSign")
            if (debugSign != null && debugSign.storeFile?.exists() == true) {
                signingConfig = debugSign
            } else {
                logger.warn("没有找到 debug keystore，release APK 将是未签名的（装不上真机）。")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
