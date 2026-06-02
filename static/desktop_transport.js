(function() {
    'use strict';

    function requestId(prefix) {
        if (window.crypto && typeof window.crypto.randomUUID === 'function') {
            return prefix + window.crypto.randomUUID();
        }
        return prefix + String(Date.now()) + '-' + String(Math.random()).slice(2);
    }

    function loopbackToken() {
        return window.MYCELA_HTTP_TOKEN || null;
    }

    function isIpcTransport() {
        return typeof window.ipc !== 'undefined' && typeof window.ipc.postMessage === 'function';
    }

    function mapPathToCommand(method, path) {
        const normalizedMethod = String(method || 'get').toLowerCase();
        if (normalizedMethod === 'post' && path === '/api/server/start') return 'epics_server_start';
        if (normalizedMethod === 'post' && path === '/api/server/stop') return 'epics_server_stop';
        if (normalizedMethod === 'get' && path === '/api/server/status') return 'epics_server_status_get';
        if (normalizedMethod === 'post' && path === '/api/modbus/start') return 'modbus_sim_start';
        if (normalizedMethod === 'post' && path === '/api/modbus/stop') return 'modbus_sim_stop';
        if (normalizedMethod === 'get' && path === '/api/modbus/status') return 'modbus_sim_status_get';
        return null;
    }

    function parseWidgetWritePath(path) {
        const match = /^\/api\/widget\/([^/]+)\/set$/.exec(String(path || ''));
        if (!match) {
            return null;
        }
        return {
            widgetId: decodeURIComponent(match[1])
        };
    }

    function parseHxVals(value) {
        if (!value) {
            return null;
        }
        try {
            return JSON.parse(value);
        } catch (_error) {
            return null;
        }
    }

    function resolveWidgetStatusTarget(element) {
        const container = element && element.closest('.widget-inner');
        return container ? container.querySelector('.status') : null;
    }

    function setStatusHtml(element, html) {
        const target = resolveWidgetStatusTarget(element);
        if (target) {
            target.innerHTML = html || '';
        }
    }

    function updateSelectDisplay(select) {
        const wrapper = select && select.closest('.select-wrapper');
        const display = wrapper ? wrapper.querySelector('.select-display-text') : null;
        if (!display) {
            return;
        }

        const option = select.options[select.selectedIndex];
        display.textContent = option ? option.textContent : select.value;
    }

    function validateWidgetInput(element) {
        if (!(element instanceof HTMLInputElement) || element.type !== 'number') {
            return true;
        }

        const parsed = Number(element.value);
        if (Number.isFinite(parsed)) {
            return true;
        }

        const previousValue = element.dataset.confirmed || element.dataset.originalValue || '';
        element.value = previousValue;
        if (element.dataset.confirmed && element.previousElementSibling && element.previousElementSibling.matches('input[type="range"]')) {
            element.previousElementSibling.value = previousValue;
        }
        setStatusHtml(element, '<span class="write-err">Invalid number</span>');
        return false;
    }

    function finalizeWidgetInput(element) {
        if (!(element instanceof HTMLInputElement)) {
            return;
        }

        if (Object.prototype.hasOwnProperty.call(element.dataset, 'originalValue')) {
            element.dataset.originalValue = element.value;
        }

        if (Object.prototype.hasOwnProperty.call(element.dataset, 'confirmed')) {
            element.dataset.confirmed = element.value;
            if (element.previousElementSibling && element.previousElementSibling.matches('input[type="range"]')) {
                element.previousElementSibling.value = element.value;
            }
        }
    }

    function renderStatus(path, result) {
        if (path === '/api/server/status') {
            return '<div id="server-status" class="' + (result.running ? 'success' : 'warning') + ' screen-status-pill"><span>' + (result.running ? 'EPICS Server Running' : 'EPICS Server Stopped') + '</span></div>';
        }
        if (path === '/api/modbus/status') {
            return '<div id="modbus-status" class="' + (result.running ? 'success' : 'warning') + ' screen-status-pill"><span>' + (result.running ? 'Modbus TCP Running' : 'Modbus TCP Stopped') + '</span></div>';
        }
        return '<div class="warning">Unknown status path</div>';
    }

    function renderActionFeedback(path, ok, error) {
        if (!ok) {
            return '<div class="error">' + (error || 'Request failed') + '</div>';
        }
        if (path === '/api/server/start') return '<div class="success">EPICS Server Running</div>';
        if (path === '/api/server/stop') return '<div class="warning">EPICS Server Stopped</div>';
        if (path === '/api/modbus/start') return '<div class="success">Modbus TCP Running</div>';
        if (path === '/api/modbus/stop') return '<div class="warning">Modbus TCP Stopped</div>';
        return '<div class="success">Request completed</div>';
    }

    function deliverStatus(path, response, element) {
        if (!element) {
            return;
        }
        if (!response.ok || !response.result) {
            element.outerHTML = '<div class="error screen-status-pill">Status unavailable</div>';
            return;
        }
        element.outerHTML = renderStatus(path, response.result);
    }

    function deliverActionResult(path, response, feedbackTarget) {
        if (feedbackTarget) {
            feedbackTarget.innerHTML = renderActionFeedback(
                path,
                response.ok,
                response.error && response.error.message
            );
        }

        if (path.indexOf('/api/server/') === 0) {
            const status = document.getElementById('server-status');
            if (status) {
                runStatusRequest(status);
            }
        }
        if (path.indexOf('/api/modbus/') === 0) {
            const status = document.getElementById('modbus-status');
            if (status) {
                runStatusRequest(status);
            }
        }
    }

    function deliverWidgetWriteResult(element, response) {
        const html = response && response.result && response.result.html;
        if (typeof html === 'string') {
            setStatusHtml(element, html);
        } else if (!response.ok) {
            setStatusHtml(element, '<span class="write-err">' + ((response.error && response.error.message) || 'Write failed') + '</span>');
        }

        if (response.ok) {
            finalizeWidgetInput(element);
            if (element instanceof HTMLSelectElement) {
                updateSelectDisplay(element);
            }
        }
    }

    function deliverWidgetEvent(event) {
        if (!event || event.event !== 'widget_html' || !event.data) {
            return;
        }

        const widgetId = event.data.widget_id;
        const html = event.data.html;
        if (!widgetId || typeof html !== 'string') {
            return;
        }

        const widget = document.querySelector('[data-widget-id="' + widgetId + '"]');
        if (!widget) {
            return;
        }

        widget.innerHTML = html;
        bindWidgetWrites(widget);
    }

    function ipcRequest(method, path, element, feedbackTarget) {
        const command = mapPathToCommand(method, path);
        if (!command) {
            if (feedbackTarget) {
                feedbackTarget.innerHTML = '<div class="error">No IPC command mapping for ' + path + '</div>';
            }
            return;
        }

        const request = {
            v: 1,
            kind: 'request',
            id: requestId('transport-'),
            cmd: command,
            token: window.MYCELA_IPC_TOKEN || null,
            payload: {},
            ts: Date.now()
        };

        if (!window.__MYCELA_TRANSPORT_PENDING) {
            window.__MYCELA_TRANSPORT_PENDING = new Map();
        }

        window.__MYCELA_TRANSPORT_PENDING.set(request.id, function(response) {
            if (String(method).toLowerCase() === 'get') {
                deliverStatus(path, response, element);
            } else {
                deliverActionResult(path, response, feedbackTarget);
            }
        });

        window.ipc.postMessage(JSON.stringify(request));
    }

    function ipcWidgetWrite(element, value) {
        const path = element.getAttribute('hx-post');
        const route = parseWidgetWritePath(path);
        if (!route) {
            setStatusHtml(element, '<span class="write-err">Invalid widget path</span>');
            return;
        }

        const request = {
            v: 1,
            kind: 'request',
            id: requestId('widget-'),
            cmd: 'app_widget_write',
            token: window.MYCELA_IPC_TOKEN || null,
            payload: {
                widget_id: route.widgetId,
                value: String(value)
            },
            ts: Date.now()
        };

        if (!window.__MYCELA_TRANSPORT_PENDING) {
            window.__MYCELA_TRANSPORT_PENDING = new Map();
        }

        window.__MYCELA_TRANSPORT_PENDING.set(request.id, function(response) {
            deliverWidgetWriteResult(element, response);
        });

        window.ipc.postMessage(JSON.stringify(request));
    }

    function httpRequest(method, path, element, feedbackTarget) {
        const normalizedMethod = String(method || 'get').toUpperCase();
        const headers = {};
        const token = loopbackToken();
        if (token && normalizedMethod !== 'GET') {
            headers['x-mycela-session-token'] = token;
        }

        fetch(path, { method: normalizedMethod, headers: headers })
            .then(function(response) { return response.text(); })
            .then(function(html) {
                if (String(method).toLowerCase() === 'get') {
                    if (element) {
                        element.outerHTML = html;
                    }
                } else if (feedbackTarget) {
                    feedbackTarget.innerHTML = html;
                }
            })
            .catch(function(error) {
                if (feedbackTarget) {
                    feedbackTarget.innerHTML = '<div class="error">' + error.message + '</div>';
                }
            });
    }

    function runStatusRequest(element) {
        const path = element.getAttribute('data-myce-status-path');
        const method = element.getAttribute('data-myce-method') || 'get';
        if (!path) {
            return;
        }
        if (isIpcTransport()) {
            ipcRequest(method, path, element, null);
        } else {
            httpRequest(method, path, element, null);
        }
    }

    function bindActionButton(button) {
        button.addEventListener('click', function() {
            const path = button.getAttribute('data-myce-api-path');
            const method = button.getAttribute('data-myce-method') || 'get';
            const feedbackSelector = button.getAttribute('data-myce-target');
            const feedbackTarget = feedbackSelector ? document.querySelector(feedbackSelector) : null;

            if (isIpcTransport()) {
                ipcRequest(method, path, null, feedbackTarget);
            } else {
                httpRequest(method, path, null, feedbackTarget);
            }
        });
    }

    function handleWidgetWriteEvent(element, event, value) {
        if (!isIpcTransport()) {
            return;
        }

        event.preventDefault();
        event.stopPropagation();

        if (!validateWidgetInput(element)) {
            return;
        }

        ipcWidgetWrite(element, value);
    }

    function bindWidgetWrite(element) {
        if (element.dataset.myceWriteBound === '1') {
            return;
        }

        const path = element.getAttribute('hx-post');
        if (!parseWidgetWritePath(path)) {
            return;
        }

        element.dataset.myceWriteBound = '1';

        if (element instanceof HTMLButtonElement) {
            element.addEventListener('click', function(event) {
                const values = parseHxVals(element.getAttribute('hx-vals'));
                const value = values && Object.prototype.hasOwnProperty.call(values, 'value')
                    ? values.value
                    : element.value;
                handleWidgetWriteEvent(element, event, value);
            }, true);
            return;
        }

        if (element instanceof HTMLSelectElement) {
            element.addEventListener('change', function(event) {
                handleWidgetWriteEvent(element, event, element.value);
            }, true);
            return;
        }

        if (element instanceof HTMLInputElement) {
            element.addEventListener('keyup', function(event) {
                if (event.key !== 'Enter') {
                    return;
                }
                handleWidgetWriteEvent(element, event, element.value);
            }, true);
        }
    }

    function bindWidgetWrites(root) {
        const scope = root || document;
        scope.querySelectorAll('[hx-post^="/api/widget/"]').forEach(bindWidgetWrite);
    }

    function subscribeCurrentScreen() {
        if (!isIpcTransport()) {
            return;
        }

        const screenId = document.body && document.body.getAttribute('data-myce-screen-id');
        if (!screenId) {
            return;
        }

        if (!window.__MYCELA_PAGE_SUBSCRIPTION_ID) {
            window.__MYCELA_PAGE_SUBSCRIPTION_ID = requestId('page-');
        }
        window.__MYCELA_PAGE_SUBSCRIPTION_CLOSED = false;

        const request = {
            v: 1,
            kind: 'request',
            id: requestId('screen-sub-'),
            cmd: 'app_screen_subscribe',
            token: null,
            payload: {
                screen_id: screenId,
                subscription_id: window.__MYCELA_PAGE_SUBSCRIPTION_ID
            },
            ts: Date.now()
        };

        if (!window.__MYCELA_TRANSPORT_PENDING) {
            window.__MYCELA_TRANSPORT_PENDING = new Map();
        }

        window.__MYCELA_TRANSPORT_PENDING.set(request.id, function(response) {
            if (!response.ok) {
                const feedback = document.getElementById('screen-action-feedback');
                if (feedback) {
                    feedback.innerHTML = '<div class="error">' + ((response.error && response.error.message) || 'Screen subscription failed') + '</div>';
                }
            }
        });

        window.ipc.postMessage(JSON.stringify(request));
    }

    function unsubscribeCurrentScreen() {
        if (!isIpcTransport() || window.__MYCELA_PAGE_SUBSCRIPTION_CLOSED) {
            return;
        }

        const screenId = document.body && document.body.getAttribute('data-myce-screen-id');
        const subscriptionId = window.__MYCELA_PAGE_SUBSCRIPTION_ID;
        if (!screenId || !subscriptionId) {
            return;
        }

        window.__MYCELA_PAGE_SUBSCRIPTION_CLOSED = true;
        window.ipc.postMessage(JSON.stringify({
            v: 1,
            kind: 'request',
            id: requestId('screen-unsub-'),
            cmd: 'app_screen_unsubscribe',
            token: null,
            payload: {
                screen_id: screenId,
                subscription_id: subscriptionId
            },
            ts: Date.now()
        }));
    }

    function navigateTo(target) {
        if (!isIpcTransport()) {
            window.location = target;
            return;
        }

        unsubscribeCurrentScreen();
        window.setTimeout(function() {
            window.location = target;
        }, 0);
    }

    function bootstrap() {
        if (!window.__MYCELA_IPC_DELIVER) {
            window.__MYCELA_IPC_DELIVER = function(response) {
                const pending = window.__MYCELA_TRANSPORT_PENDING && window.__MYCELA_TRANSPORT_PENDING.get(response.id);
                if (pending) {
                    window.__MYCELA_TRANSPORT_PENDING.delete(response.id);
                    pending(response);
                }
            };
        } else {
            const previousDeliver = window.__MYCELA_IPC_DELIVER;
            window.__MYCELA_IPC_DELIVER = function(response) {
                const pending = window.__MYCELA_TRANSPORT_PENDING && window.__MYCELA_TRANSPORT_PENDING.get(response.id);
                if (pending) {
                    window.__MYCELA_TRANSPORT_PENDING.delete(response.id);
                    pending(response);
                }
                previousDeliver(response);
            };
        }

        if (!window.__MYCELA_IPC_EVENT_DELIVER) {
            window.__MYCELA_IPC_EVENT_DELIVER = function(event) {
                deliverWidgetEvent(event);
            };
        } else {
            const previousEventDeliver = window.__MYCELA_IPC_EVENT_DELIVER;
            window.__MYCELA_IPC_EVENT_DELIVER = function(event) {
                deliverWidgetEvent(event);
                previousEventDeliver(event);
            };
        }

        window.__MYCELA_NAVIGATE = navigateTo;

        if (!isIpcTransport() && window.htmx && loopbackToken()) {
            document.body.addEventListener('htmx:configRequest', function(event) {
                const verb = String(event.detail.verb || '').toUpperCase();
                if (verb !== 'GET') {
                    event.detail.headers['x-mycela-session-token'] = loopbackToken();
                }
            });
        }

        document.querySelectorAll('[data-myce-api-path]').forEach(bindActionButton);
        document.querySelectorAll('[data-myce-status-path]').forEach(runStatusRequest);
        bindWidgetWrites(document);
        window.addEventListener('beforeunload', unsubscribeCurrentScreen, { once: true });
        window.addEventListener('pagehide', unsubscribeCurrentScreen, { once: true });
        subscribeCurrentScreen();
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', bootstrap, { once: true });
    } else {
        bootstrap();
    }
})();