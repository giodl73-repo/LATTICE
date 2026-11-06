import {defineConfig} from '@playwright/test';
const live=process.env.LATTICE_SITE_URL;
export default defineConfig({
 testDir:'./tests/browser',timeout:60000,workers:2,retries:0,
 use:{baseURL:live||'http://127.0.0.1:8771',browserName:'chromium',launchOptions:process.env.LATTICE_BROWSER_PATH?{executablePath:process.env.LATTICE_BROWSER_PATH}:{}},
 webServer:live?undefined:{command:'python tools/serve-pages.py',url:'http://127.0.0.1:8771/LATTICE/',reuseExistingServer:false},
});
