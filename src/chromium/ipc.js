const createIpc = () => {
    let listeners = [];

    globalThis.IPC_SENDER = (data) => {
        listeners.forEach((listener) => {
            listener({ data });
        });
    };

    const postMessage = (data) => {
        globalThis.IPC_RECEIVER(data);
    };

    const addEventListener = (name, listener) => {
        if (name !== 'message')
            throw Error('Unsupported event');

        listeners.push(listener);
    };

    const removeEventListener = (name, listener) => {
        if (name !== 'message')
            throw Error('Unsupported event');

        listeners = listeners.filter((it) => it !== listener);
    };

    return {
        postMessage,
        addEventListener,
        removeEventListener,
    };
};

window.ipc = createIpc();

// Backward compatibility
window.qt = {
    webChannelTransport: {
        send: window.ipc.postMessage,
    },
};

globalThis.chrome = {
    webview: {
        postMessage: window.ipc.postMessage,
        addEventListener: (name, listener) => {
            window.ipc.addEventListener(name, listener);
        },
        removeEventListener: (name, listener) => {
            window.ipc.removeEventListener(name, listener);
        },
    },
};

window.ipc.addEventListener('message', (message) => {
    window.qt.webChannelTransport.onmessage(message);
});

console.log('IPC script injected');

// Metadata Scraper Logic
(function () {
    let lastTitle = "";
    let lastPoster = "";
    let lastLogo = "";

    let servicesHooked = false;
    let internalMetadata = { title: "", artist: "", poster: "", logo: "" };

    function hookServices() {
        if (!servicesHooked && window.services && window.services.core) {
            try {
                servicesHooked = true;
                setInterval(async () => {
                    try {
                        if (window.services && window.services.core && window.services.core.transport) {
                            const state = await window.services.core.transport.getState('player');
                            if (state && state.event && state.event.name === 'video-changed') {
                                internalMetadata = { title: "", artist: "", poster: "", logo: "" };
                            }
                            if (state && state.metaItem) {
                                let seriesName = state.metaItem.name || "";
                                let epTitle = "";
                                let art = "";

                                // 1. Try to find the specific video (Episode)
                                if (state.selected && state.selected.streamRequest && state.selected.streamRequest.path) {
                                    const vidId = state.selected.streamRequest.path.id;
                                    const video = state.metaItem.videos.find(v => v.id === vidId);

                                    if (video) {
                                        if (video.thumbnail) {
                                            art = video.thumbnail;
                                        } else if (video.thumbnailUrl) {
                                            art = video.thumbnailUrl;
                                        }

                                        if (video.title) {
                                            epTitle = video.title;
                                            if (video.season && video.episode) {
                                                if (!epTitle.includes(video.season + "x")) {
                                                    epTitle = `${video.season}x${video.episode} ${epTitle}`;
                                                }
                                            }
                                        } else {
                                            if (video.season && video.episode) {
                                                epTitle = `${seriesName} (${video.season}x${video.episode})`;
                                            }
                                        }
                                    }
                                }

                                if (!art) {
                                    if (state.metaItem.background) {
                                        art = state.metaItem.background;
                                    } else {
                                        art = state.metaItem.logo;
                                    }
                                }

                                if (state.metaItem.logo) {
                                    internalMetadata.logo = state.metaItem.logo;
                                }

                                internalMetadata.title = epTitle;
                                internalMetadata.artist = seriesName;
                                internalMetadata.poster = art; // Map 'art' to 'poster' for IPC
                            }
                        }
                    } catch (err) { }
                }, 2000);
            } catch (e) {
                console.error("Failed to hook services", e);
            }
        }
    }

    function checkMetadata() {
        hookServices();

        let title = document.title;
        let artist = "";
        let poster = "";
        let logo = "";

        // Source 1: Internal State
        if (internalMetadata.poster) poster = internalMetadata.poster;
        if (internalMetadata.title) title = internalMetadata.title;
        if (internalMetadata.artist) artist = internalMetadata.artist;
        if (internalMetadata.logo) logo = internalMetadata.logo;

        // Source 2: MediaSession API
        if (!poster && navigator.mediaSession && navigator.mediaSession.metadata && navigator.mediaSession.metadata.artwork.length > 0) {
            poster = navigator.mediaSession.metadata.artwork[0].src;
        }
        if (!artist && navigator.mediaSession && navigator.mediaSession.metadata && navigator.mediaSession.metadata.artist) {
            artist = navigator.mediaSession.metadata.artist;
        }
        if (navigator.mediaSession && navigator.mediaSession.metadata && navigator.mediaSession.metadata.title) {
            title = navigator.mediaSession.metadata.title;
        }

        // Source 3: DOM scraping fallback
        if (!title || title.trim().toLowerCase() === "stremio") {
            const titleSelectors = ['.nav-bar-layer', '.player-title', '.video-title', '.meta-title', 'h1'];
            for (const selector of titleSelectors) {
                const el = document.querySelector(selector);
                if (el && el.innerText && el.innerText.trim().length > 0) {
                    title = el.innerText.trim();
                    break;
                }
            }
        }

        if (!poster) {
            const selectors = ['.player-poster img', '.meta-poster img', 'img[class*="poster"]', 'img[src*="poster"]'];
            for (const selector of selectors) {
                const el = document.querySelector(selector);
                if (el && el.src) {
                    poster = el.src;
                    break;
                }
            }
        }

        if (!logo) {
            const logoSelectors = ['img[src*="logo"]', '.logo-container img', '.logo img'];
            for (const selector of logoSelectors) {
                const el = document.querySelector(selector);
                if (el && el.src) {
                    logo = el.src;
                    break;
                }
            }
        }

        if (title !== lastTitle || poster !== lastPoster || logo !== lastLogo) {
            lastTitle = title;
            lastPoster = poster;
            lastLogo = logo;

            // Send to Rust
            // Note: We map 'poster' (which holds the artwork/thumbnail URL) to 'poster' field.
            // Rust 'UserEvent::MetadataUpdate' has 'poster', 'thumbnail', and 'logo'.
            // MprisAdapter uses 'thumbnail' or 'poster'.
            // Here we send 'poster' and 'thumbnail' as the same value (the artwork) to cover bases, or just 'poster'.
            // Rust bridge maps 'data.poster' -> 'UserEvent.poster' and 'data.thumbnail' -> 'UserEvent.thumbnail'.
            // Let's send both or just one?
            // In clean_version, JS sent "art_url".
            // In my new struct, I have "poster" and "thumbnail".
            // I will populate "poster" with the main artwork.

            window.ipc.postMessage(JSON.stringify({
                id: Date.now(),
                type: 6,
                args: ["metadata-update", {
                    title: title,
                    artist: artist,
                    poster: poster,
                    thumbnail: poster, // Send same URL as thumbnail for MPRIS preference
                    logo: logo
                }]
            }));
        }
    }

    setInterval(checkMetadata, 2000);
})();
