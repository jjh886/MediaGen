Name: mediagen
Version: 0.1.0
Release: 1%{?dist}
Summary: MediaGen Web Console server
License: Open Source

%description
MediaGen Web Console server with plugin execution, browser sessions, and CloakBrowser runtime.

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/payload/* %{buildroot}/

%post
id mediagen >/dev/null 2>&1 || useradd --system --home /var/lib/mediagen --shell /sbin/nologin mediagen
mkdir -p /var/lib/mediagen/profiles /var/log/mediagen
chown -R mediagen:mediagen /var/lib/mediagen /var/log/mediagen
systemctl daemon-reload || true
systemctl enable mediagen-server.service || true

%preun
systemctl stop mediagen-server.service || true

%postun
systemctl daemon-reload || true

%files
/usr/bin/mediagen-server
/usr/share/mediagen/console
/opt/mediagen/plugins
/opt/mediagen/browser-runtime
/lib/systemd/system/mediagen-server.service
/etc/mediagen/mediagen.env
