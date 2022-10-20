use futures::stream::{self, StreamExt};
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    sync::Arc,
};
use zbus::{
    fdo,
    fdo::ManagedObjects,
    names::{BusName, OwnedUniqueName, UniqueName, WellKnownName},
    Connection, OwnerChangedStream,
};
use zvariant::OwnedObjectPath;

#[cfg(unix)]
use crate::UsbRedir;
use crate::{Audio, Chardev, Clipboard, Error, Result, VMProxy};

#[cfg(all(unix, feature = "qmp"))]
use std::os::unix::net::UnixStream;
#[cfg(all(windows, feature = "qmp"))]
use uds_windows::UnixStream;

struct Inner<'d> {
    proxy: fdo::ObjectManagerProxy<'d>,
    conn: Connection,
    objects: ManagedObjects,
    #[cfg(windows)]
    peer_pid: u32,
}

#[derive(Clone)]
pub struct Display<'d> {
    inner: Arc<Inner<'d>>,
}

impl<'d> Display<'d> {
    pub async fn lookup(
        conn: &Connection,
        wait: bool,
        name: Option<&str>,
    ) -> Result<Option<OwnedUniqueName>> {
        let mut changed = fdo::DBusProxy::new(conn)
            .await?
            .receive_name_owner_changed()
            .await?;
        loop {
            let list = Display::by_name(conn).await?;
            if let Some(name) = name {
                let res = list.get(name);
                if res.is_some() {
                    return Ok(res.cloned());
                }
            } else if !list.is_empty() {
                return Ok(None);
            }
            if !wait {
                return Err(Error::Failed("Can't find VM".into()));
            };
            let _ = changed.next().await;
        }
    }

    pub async fn by_name(conn: &Connection) -> Result<HashMap<String, OwnedUniqueName>> {
        let mut hm = HashMap::new();
        let list = match fdo::DBusProxy::new(conn)
            .await?
            .list_queued_owners(WellKnownName::from_str_unchecked("org.qemu"))
            .await
        {
            Ok(list) => list,
            Err(zbus::fdo::Error::NameHasNoOwner(_)) => vec![],
            Err(e) => return Err(e.into()),
        };
        for dest in list.into_iter() {
            let name = VMProxy::builder(conn)
                .destination(UniqueName::from(&dest))?
                .build()
                .await?
                .name()
                .await?;
            hm.insert(name, dest);
        }
        Ok(hm)
    }

    pub async fn new<D>(
        conn: &Connection,
        dest: Option<D>,
        #[cfg(windows)] peer_pid: u32,
    ) -> Result<Display<'d>>
    where
        D: TryInto<BusName<'d>>,
        D::Error: Into<Error>,
    {
        let builder = fdo::ObjectManagerProxy::builder(conn);
        let builder = if let Some(dest) = dest {
            let dest = dest.try_into().map_err(Into::into)?;
            builder.destination(dest)?
        } else {
            builder
        };
        let proxy = builder.path("/org/qemu/Display1")?.build().await?;
        let objects = proxy.get_managed_objects().await?;
        // TODO: listen for changes
        let inner = Inner {
            // owner_changed,
            proxy,
            conn: conn.clone(),
            objects,
            #[cfg(windows)]
            peer_pid,
        };

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.inner.conn
    }

    #[cfg(windows)]
    pub fn peer_pid(&self) -> u32 {
        self.inner.peer_pid
    }

    #[cfg(all(windows, feature = "qmp"))]
    pub async fn new_qmp<P: AsRef<std::path::Path>>(path: P) -> Result<Display<'d>> {
        #![allow(non_snake_case, non_camel_case_types)]

        use crate::win32::{duplicate_socket, unix_stream_get_peer_pid};
        use qapi::{qmp, Qmp};
        use serde::{Deserialize, Serialize};
        use std::os::windows::io::AsRawSocket;
        use windows::Win32::Networking::WinSock::SOCKET;

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct get_win32_socket {
            #[serde(rename = "info")]
            pub info: ::std::string::String,
            #[serde(rename = "fdname")]
            pub fdname: ::std::string::String,
        }

        impl qmp::QmpCommand for get_win32_socket {}
        impl qapi::Command for get_win32_socket {
            const NAME: &'static str = "get-win32-socket";
            const ALLOW_OOB: bool = false;

            type Ok = qapi::Empty;
        }

        let stream = UnixStream::connect(path)?;
        let pid = unix_stream_get_peer_pid(&stream)?;
        let mut qmp = Qmp::from_stream(&stream);
        let _info = qmp.handshake()?;

        let (p0, p1) = UnixStream::pair()?;
        let info = duplicate_socket(pid, SOCKET(p0.as_raw_socket() as _))?;
        let info = base64::encode(info);
        qmp.execute(&get_win32_socket {
            info,
            fdname: "fdname".into(),
        })?;
        qmp.execute(&qmp::add_client {
            skipauth: None,
            tls: None,
            protocol: "@dbus-display".into(),
            fdname: "fdname".into(),
        })?;

        let conn = zbus::ConnectionBuilder::unix_stream(p1)
            .p2p()
            .build()
            .await?;

        Self::new(&conn, Option::<String>::None, pid).await
    }

    pub async fn receive_owner_changed(&self) -> Result<OwnerChangedStream<'_>> {
        Ok(self.inner.proxy.receive_owner_changed().await?)
    }

    pub async fn audio(&self) -> Result<Option<Audio>> {
        if !self
            .inner
            .objects
            .contains_key(&OwnedObjectPath::try_from("/org/qemu/Display1/Audio").unwrap())
        {
            return Ok(None);
        }

        Ok(Some(
            Audio::new(
                &self.inner.conn,
                #[cfg(windows)]
                self.peer_pid(),
            )
            .await?,
        ))
    }

    pub async fn clipboard(&self) -> Result<Option<Clipboard>> {
        if !self
            .inner
            .objects
            .contains_key(&OwnedObjectPath::try_from("/org/qemu/Display1/Clipboard").unwrap())
        {
            return Ok(None);
        }

        Ok(Some(Clipboard::new(&self.inner.conn).await?))
    }

    pub async fn chardevs(&self) -> Vec<Chardev> {
        stream::iter(&self.inner.objects)
            .filter_map(|(p, _ifaces)| async move {
                match p.strip_prefix("/org/qemu/Display1/Chardev_") {
                    Some(id) => Chardev::new(&self.inner.conn, id).await.ok(),
                    _ => None,
                }
            })
            .collect()
            .await
    }

    #[cfg(unix)]
    pub async fn usbredir(&self) -> UsbRedir {
        let chardevs = stream::iter(self.chardevs().await)
            .filter_map(|c| async move {
                if c.proxy.name().await.ok() == Some("org.qemu.usbredir".to_string()) {
                    Some(c)
                } else {
                    None
                }
            })
            .collect()
            .await;

        UsbRedir::new(chardevs)
    }
}
