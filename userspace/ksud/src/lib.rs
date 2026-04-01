use jni::EnvUnowned;
use jni::objects::JClass;
use log::error;

#[cfg(target_os = "android")]
mod android;
use crate::android::magica::run;

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_resukisu_resukisu_magica_AppZygotePreload_executeMagica(
    _: EnvUnowned,
    _: JClass,
) {
    if let Err(e) = run(5555) {
        error!("Error running magica: {e}");
    }
}