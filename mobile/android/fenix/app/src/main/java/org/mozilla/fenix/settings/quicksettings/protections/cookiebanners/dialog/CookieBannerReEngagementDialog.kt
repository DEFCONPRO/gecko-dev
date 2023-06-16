/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package org.mozilla.fenix.settings.quicksettings.protections.cookiebanners.dialog

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.platform.ComposeView
import androidx.fragment.app.DialogFragment
import mozilla.components.concept.engine.EngineSession.CookieBannerHandlingMode.DISABLED
import mozilla.components.concept.engine.EngineSession.CookieBannerHandlingMode.REJECT_ALL
import mozilla.components.concept.engine.Settings
import mozilla.telemetry.glean.private.NoExtras
import org.mozilla.fenix.GleanMetrics.CookieBanners
import org.mozilla.fenix.R
import org.mozilla.fenix.components.FenixSnackbar
import org.mozilla.fenix.ext.components
import org.mozilla.fenix.ext.getRootView
import org.mozilla.fenix.ext.settings
import org.mozilla.fenix.theme.FirefoxTheme

/**
 * Displays a cookie banner dialog fragment that contains the dialog compose and his logic.
 */
class CookieBannerReEngagementDialog : DialogFragment() {

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?,
    ): View = ComposeView(requireContext()).apply {
        CookieBanners.visitedReEngagementDialog.record(NoExtras())

        setContent {
            FirefoxTheme {
                val title =
                    context.getString(
                        R.string.reduce_cookie_banner_dialog_title,
                        context.getString(R.string.app_name),
                    )

                val message =
                    context.getString(
                        R.string.reduce_cookie_banner_dialog_body,
                        context.getString(R.string.app_name),
                    )

                val allowButtonText =
                    context.getString(
                        R.string.reduce_cookie_banner_dialog_change_setting_button,
                    )

                CookieBannerReEngagementDialogCompose(
                    dialogTitle = title,
                    dialogText = message,
                    allowButtonText = allowButtonText,
                    declineButtonText = getString(R.string.reduce_cookie_banner_dialog_not_now_button),
                    onAllowButtonClicked = {
                        CookieBanners.allowReEngagementDialog.record(NoExtras())
                        requireContext().settings().shouldUseCookieBanner = true
                        getEngineSettings().cookieBannerHandlingModePrivateBrowsing = REJECT_ALL
                        getEngineSettings().cookieBannerHandlingMode = REJECT_ALL
                        getEngineSettings().cookieBannerHandlingDetectOnlyMode = false
                        reload()
                        requireContext().getRootView()?.let {
                            FenixSnackbar.make(
                                view = it,
                                duration = LENGTH_SNACKBAR_DURATION,
                                isDisplayedWithBrowserToolbar = true,
                            )
                                .setText(getString(R.string.reduce_cookie_banner_dialog_snackbar_text))
                                .show()
                        }
                        dismiss()
                    },
                    onNotNowButtonClicked = {
                        disabledCookieBannerHandlingDetectOnlyMode()
                        CookieBanners.notNowReEngagementDialog.record(NoExtras())
                        dismiss()
                    },
                    onCloseButtonClicked = {
                        disabledCookieBannerHandlingDetectOnlyMode()
                        requireContext().settings().userOptOutOfReEngageCookieBannerDialog = true
                        CookieBanners.optOutReEngagementDialog.record(NoExtras())
                        dismiss()
                    },
                )
            }
        }
    }

    private fun disabledCookieBannerHandlingDetectOnlyMode() {
        getEngineSettings().cookieBannerHandlingDetectOnlyMode = false
        getEngineSettings().cookieBannerHandlingModePrivateBrowsing = DISABLED
        getEngineSettings().cookieBannerHandlingMode = DISABLED
    }

    private fun getEngineSettings(): Settings {
        return requireContext().components.core.engine.settings
    }

    private fun reload() {
        return requireContext().components.useCases.sessionUseCases.reload()
    }

    companion object {
        private const val LENGTH_SNACKBAR_DURATION = 4000 // 4 seconds in ms
    }
}
