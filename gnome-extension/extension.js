import { Extension, gettext as _ } from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import St from 'gi://St';
import GObject from 'gi://GObject';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';

const DBUS_INTERFACE = `
<node>
  <interface name="com.pais.Handy.Status">
    <property name="Status" type="s" access="read"/>
    <method name="GetStatus">
      <arg type="s" name="status" direction="out"/>
    </method>
    <signal name="StatusChanged">
      <arg type="s" name="status"/>
    </signal>
  </interface>
</node>
`;

const HandyIndicator = GObject.registerClass(
    class HandyIndicator extends PanelMenu.Button {
        _init() {
            super._init(0.0, _('Handy Status'));

            this._proxy = null;
            this._watcherId = 0;
            this._status = 'idle';

            this._buildUi();
            this._watchDbus();
        }

        _buildUi() {
            this._icon = new St.Icon({
                style_class: 'handy-status-icon',
                icon_name: 'media-record-symbolic',
            });
            this.add_child(this._icon);

            this._updateVisibility();
            this._updateStyle();
        }

        _watchDbus() {
            const ProxyClass = Gio.DBusProxy.makeProxyWrapper(DBUS_INTERFACE);

            this._watcherId = Gio.bus_watch_name(
                Gio.BusType.SESSION,
                'com.pais.Handy',
                Gio.BusNameWatcherFlags.NONE,
                (connection, name, owner) => {
                    this._proxy = new ProxyClass(
                        connection,
                        'com.pais.Handy',
                        '/com/pais/Handy'
                    );
                    this._proxy.connectSignal('StatusChanged', (proxy, sender, [status]) => {
                        this._setStatus(status);
                    });
                    this._syncStatus();
                },
                () => {
                    this._proxy = null;
                    this._setStatus('idle');
                }
            );
        }

        _syncStatus() {
            if (!this._proxy) {
                this._setStatus('idle');
                return;
            }
            try {
                const status = this._proxy.GetStatusSync();
                this._setStatus(status);
            } catch (e) {
                this._setStatus('idle');
            }
        }

        _setStatus(status) {
            if (this._status === status) {
                return;
            }
            this._status = status;
            this._updateVisibility();
            this._updateStyle();
        }

        _updateVisibility() {
            this.visible = this._status !== 'idle';
        }

        _updateStyle() {
            this._icon.remove_style_class_name('handy-recording');
            this._icon.remove_style_class_name('handy-transcribing');
            this._icon.remove_style_class_name('handy-processing');

            switch (this._status) {
                case 'recording':
                    this._icon.icon_name = 'media-record-symbolic';
                    this._icon.add_style_class_name('handy-recording');
                    break;
                case 'transcribing':
                    this._icon.icon_name = 'content-loading-symbolic';
                    this._icon.add_style_class_name('handy-transcribing');
                    break;
                case 'processing':
                    this._icon.icon_name = 'emblem-synchronizing-symbolic';
                    this._icon.add_style_class_name('handy-processing');
                    break;
                default:
                    this._icon.icon_name = 'dialog-information-symbolic';
                    break;
            }
        }

        destroy() {
            if (this._watcherId) {
                Gio.bus_unwatch_name(this._watcherId);
                this._watcherId = 0;
            }
            this._proxy = null;
            super.destroy();
        }
    }
);

export default class HandyStatusExtension extends Extension {
    enable() {
        this._indicator = new HandyIndicator();
        Main.panel.addToStatusArea('handy-status', this._indicator);
    }

    disable() {
        this._indicator.destroy();
        this._indicator = null;
    }
}
