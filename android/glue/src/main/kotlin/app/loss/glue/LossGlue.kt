// Marker class for the Rust declaration in app/src/glue.rs. That declaration is what makes dx
// bundle this Gradle module (as :plugins:lossglue); its name comes from this class.

package app.loss.glue

import android.app.Activity

@Suppress("unused")
class LossGlue(private val activity: Activity)
