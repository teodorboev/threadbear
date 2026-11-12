package com.mikerosoft.threadbear.android

import android.app.Application
import kotlinx.coroutines.MainScope

class ThreadbearApplication : Application() {
    lateinit var model: AppModel
        private set

    override fun onCreate() {
        super.onCreate()
        Push.start(this)
        model = AppModel(this, MainScope())
    }
}
