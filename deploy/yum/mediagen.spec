Name: mediagen
Version: 0.1.0
Release: 1%{?dist}
Summary: MediaGen Web Console server
License: Open Source
Requires: ca-certificates
Requires: curl
Requires: tar
Requires: unzip
Requires: fontconfig
Requires: nss
Requires: gtk3
Requires: alsa-lib
Requires: libX11
Requires: libXcomposite
Requires: libXdamage
Requires: libXext
Requires: libXfixes
Requires: libXrandr
Requires: libxcb

%description
MediaGen Web Console server with plugin execution, browser sessions, and account management.

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/payload/* %{buildroot}/

%post
id mediagen >/dev/null 2>&1 || useradd --system --home /var/lib/mediagen --shell /sbin/nologin mediagen
mkdir -p /var/lib/mediagen/profiles /var/lib/mediagen/.cloakbrowser /var/log/mediagen
chown -R mediagen:mediagen /var/lib/mediagen /var/log/mediagen
systemctl daemon-reload || true
systemctl enable mediagen-server.service || true
systemctl start mediagen-server.service || true

%preun
systemctl stop mediagen-server.service || true

%postun
systemctl daemon-reload || true

%files
/usr/bin/mediagen-server
/usr/share/mediagen/console
/opt/mediagen/plugins
/opt/mediagen/browser-runtime
/opt/mediagen/ffmpeg-runtime
/lib/systemd/system/mediagen-server.service
/etc/mediagen/mediagen.env
