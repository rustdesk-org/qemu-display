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

struct Inner<'d> {
    proxy: fdo::ObjectManagerProxy<'d>,
    conn: Connection,
    objects: ManagedObjects,
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
            let list = Display::by_name(&conn).await?;
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

    pub async fn new<D>(conn: &Connection, dest: Option<D>) -> Result<Display<'d>>
    where
        D: TryInto<BusName<'d>>,
        D::Error: Into<Error>,
    {
        let dest: BusName = if let Some(dest) = dest {
            dest.try_into().map_err(Into::into)?
        } else {
            "org.qemu".try_into().unwrap()
        };
        let proxy = fdo::ObjectManagerProxy::builder(conn)
            .destination(dest)?
            .path("/org/qemu/Display1")?
            .build()
            .await?;
        let objects = proxy.get_managed_objects().await?;
        // TODO: listen for changes
        let inner = Inner {
            // owner_changed,
            proxy,
            conn: conn.clone(),
            objects,
        };

        Ok(Self {
            inner: Arc::new(inner),
        })
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

        Ok(Some(Audio::new(&self.inner.conn).await?))
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
