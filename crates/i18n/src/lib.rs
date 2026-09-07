use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Locale {
    #[default]
    English,
    French,
    Spanish,
}

impl Locale {
    pub const ALL: [Self; 3] = [Self::English, Self::French, Self::Spanish];

    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::French => "fr-FR",
            Self::Spanish => "es",
        }
    }

    pub const fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::French => "Français",
            Self::Spanish => "Español",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        let language = tag
            .trim()
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match language.as_str() {
            "en" => Some(Self::English),
            "fr" => Some(Self::French),
            "es" => Some(Self::Spanish),
            _ => None,
        }
    }

    pub fn resolve(saved: Option<&str>, system: Option<&str>) -> Self {
        saved
            .and_then(Self::from_tag)
            .or_else(|| system.and_then(Self::from_tag))
            .unwrap_or_default()
    }
}

macro_rules! catalog {
    ($locale:expr, $source:expr, { $($english:literal => ($french:literal, $spanish:literal)),* $(,)? }) => {
        match $source {
            $(
                $english => match $locale {
                    Locale::English => $english,
                    Locale::French => $french,
                    Locale::Spanish => $spanish,
                },
            )*
            other => other,
        }
    };
}

pub fn text<'a>(locale: Locale, source: &'a str) -> Cow<'a, str> {
    Cow::Borrowed(catalog!(locale, source, {
        // Common controls and states.
        "Ready" => ("Prêt", "Listo"),
        "Missing" => ("Manquante", "Falta"),
        "Checking" => ("Vérification", "Comprobando"),
        "No media" => ("Aucun média", "Sin contenido multimedia"),
        "Media detected" => ("Média détecté", "Contenido multimedia detectado"),
        "Reference image missing" => ("Image de référence manquante", "Falta la imagen de referencia"),
        "Initializing" => ("Initialisation", "Inicializando"),
        "Unavailable" => ("Indisponible", "No disponible"),
        "Failed" => ("Échec", "Error"),
        "Automatic" => ("Automatique", "Automático"),
        "Camera" => ("Caméra", "Cámara"),
        "Display" => ("Écran", "Pantalla"),
        "Webcam" => ("Webcam", "Cámara web"),
        "Screen" => ("Écran", "Pantalla"),
        "Secondary screen" => ("Écran secondaire", "Pantalla secundaria"),
        "JW Library" => ("JW Library", "JW Library"),
        "Output" => ("Sortie", "Salida"),
        "Zoom output" => ("Sortie Zoom", "Salida de Zoom"),
        "Reference" => ("Référence", "Referencia"),
        "Media detection" => ("Détection des médias", "Detección de contenido multimedia"),
        "Live" => ("EN DIRECT", "EN DIRECTO"),
        "LIVE" => ("EN DIRECT", "EN DIRECTO"),
        "WEBCAM" => ("WEBCAM", "CÁMARA WEB"),
        "SCREEN" => ("ÉCRAN", "PANTALLA"),
        "SECONDARY SCREEN" => ("ÉCRAN SECONDAIRE", "PANTALLA SECUNDARIA"),
        "JW LIBRARY" => ("JW LIBRARY", "JW LIBRARY"),
        "REFERENCE" => ("RÉFÉRENCE", "REFERENCIA"),
        "REFERENCE IMAGE" => ("IMAGE DE RÉFÉRENCE", "IMAGEN DE REFERENCIA"),
        "OUTPUT" => ("SORTIE", "SALIDA"),
        "ZOOM OUTPUT" => ("SORTIE ZOOM", "SALIDA DE ZOOM"),
        "Unknown" => ("Inconnu", "Desconocido"),
        "Crossfading" => ("Fondu enchaîné", "Fundido cruzado"),
        "Screen mix" => ("Mixage écran", "Mezcla de pantalla"),
        "Auto" => ("Auto", "Auto"),
        "Close" => ("Fermer", "Cerrar"),
        "Cancel" => ("Annuler", "Cancelar"),
        "Back" => ("Retour", "Atrás"),
        "Continue" => ("Continuer", "Continuar"),
        "Skip" => ("Ignorer", "Omitir"),

        // Dashboard and previews.
        "Settings" => ("Paramètres", "Ajustes"),
        "Components health" => ("État des composants", "Estado de los componentes"),
        "Main controls" => ("Commandes principales", "Controles principales"),
        "Automatic switching" => ("Changement automatique", "Cambio automático"),
        "Start automatic switching" => ("Activer le changement automatique", "Activar el cambio automático"),
        "Stop automatic switching" => ("Désactiver le changement automatique", "Desactivar el cambio automático"),
        "Stopping automatic switching…" => ("Désactivation du changement automatique…", "Desactivando el cambio automático…"),
        "Output mode" => ("Mode de sortie", "Modo de salida"),
        "Other" => ("Autres actions", "Otras acciones"),
        "Rescan screens" => ("Rechercher les écrans", "Buscar pantallas"),
        "No webcam frame" => ("Aucune image de webcam", "No hay imagen de la cámara web"),
        "No secondary screen frame" => ("Aucune image de l’écran secondaire", "No hay imagen de la pantalla secundaria"),
        "No reference image" => ("Aucune image de référence", "No hay imagen de referencia"),
        "No Zoom output frame" => ("Aucune image de sortie Zoom", "No hay imagen de salida de Zoom"),
        "Preparing preview…" => ("Préparation de l’aperçu…", "Preparando vista previa…"),

        // Settings shell.
        "General" => ("Général", "General"),
        "Diagnostics" => ("Diagnostic", "Diagnóstico"),
        "Choose how StageSwap starts, stays open, and alerts you." => ("Choisissez comment StageSwap démarre, reste actif et vous avertit.", "Elige cómo se inicia StageSwap, cómo permanece activo y cómo te avisa."),
        "Choose the webcam Zoom sees when JW Library is not playing media. Output is always 16:9." => ("Choisissez la webcam que Zoom voit lorsqu’aucun média n’est affiché dans JW Library. La sortie est toujours au format 16:9.", "Elige la cámara web que ve Zoom cuando JW Library no reproduce contenido multimedia. La salida siempre es 16:9."),
        "Choose the secondary screen JW Library uses for presentations. StageSwap watches it for media." => ("Choisissez l’écran secondaire utilisé par JW Library pour les présentations. StageSwap le surveille pour détecter les médias.", "Elige la pantalla secundaria que JW Library usa para las presentaciones. StageSwap la vigila para detectar contenido multimedia."),
        "Capture the screen JW Library shows when no media is playing. StageSwap compares the live screen with it to detect media." => ("Capturez l’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture. StageSwap le compare à l’écran en direct pour détecter les médias.", "Captura la pantalla que muestra JW Library cuando no se reproduce contenido multimedia. StageSwap la compara con la pantalla en directo para detectar contenido multimedia."),
        "Check video connections, troubleshoot problems, and view logs." => ("Vérifiez les connexions vidéo, résolvez les problèmes et consultez les journaux.", "Comprueba las conexiones de vídeo, soluciona problemas y consulta los registros."),
        "PREFERENCES" => ("PRÉFÉRENCES", "PREFERENCIAS"),
        "Back to dashboard" => ("Retour au tableau de bord", "Volver al panel"),
        "Could not save settings" => ("Impossible d’enregistrer les paramètres", "No se pudieron guardar los ajustes"),

        // General settings.
        "StageSwap automatically switches what Zoom sees between the webcam and JW Library presentations. When the secondary screen matches the reference image, Zoom sees the webcam. When media is detected, Zoom sees the secondary screen. When no media is detected again, Zoom returns to the webcam." => ("StageSwap alterne automatiquement ce que Zoom voit entre la webcam et les présentations JW Library. Lorsque l’écran secondaire correspond à l’image de référence, Zoom voit la webcam. Lorsqu’un média est détecté, Zoom voit l’écran secondaire. Dès qu’aucun média n’est détecté, Zoom revient à la webcam.", "StageSwap cambia automáticamente lo que ve Zoom entre la cámara web y las presentaciones de JW Library. Cuando la pantalla secundaria coincide con la imagen de referencia, Zoom ve la cámara web. Cuando se detecta contenido multimedia, Zoom ve la pantalla secundaria. Cuando deja de detectarse contenido multimedia, Zoom vuelve a la cámara web."),
        "StageSwap is an independent, unofficial project and is not affiliated with or endorsed by the publisher of JW Library. The name JW Library is used only to describe compatibility." => ("StageSwap est un projet indépendant et non officiel. Il n’est ni affilié à l’éditeur de JW Library ni approuvé par celui-ci. Le nom JW Library est utilisé uniquement pour décrire la compatibilité.", "StageSwap es un proyecto independiente y no oficial. No está afiliado ni respaldado por el editor de JW Library. El nombre JW Library se usa únicamente para describir la compatibilidad."),
        "Startup" => ("Démarrage", "Inicio"),
        "System tray" => ("Zone de notification", "Área de notificación"),
        "Choose what StageSwap does after you sign in to Windows." => ("Choisissez ce que fait StageSwap après votre connexion à Windows.", "Elige qué hace StageSwap después de iniciar sesión en Windows."),
        "Start with Windows" => ("Démarrer avec Windows", "Iniciar con Windows"),
        "Install StageSwap to use a stable Windows startup path." => ("Installez StageSwap pour utiliser un chemin de démarrage Windows stable.", "Instala StageSwap para usar una ruta de inicio de Windows estable."),
        "Install StageSwap to enable startup" => ("Installer StageSwap pour activer le démarrage", "Instalar StageSwap para activar el inicio"),
        "Launch after Windows sign-in." => ("Lancer après la connexion à Windows.", "Iniciar después de entrar en Windows."),
        "Start minimized" => ("Démarrer réduit", "Iniciar minimizado"),
        "Open in the system tray." => ("Ouvrir dans la zone de notification.", "Abrir en el área de notificación."),
        "Start automatic switching on launch" => ("Activer le changement automatique au démarrage", "Activar el cambio automático al iniciar"),
        "Start automatic switching when StageSwap launches." => ("Lancer le changement automatique au démarrage de StageSwap.", "Iniciar el cambio automático al iniciar StageSwap."),
        "Window behavior" => ("Comportement de la fenêtre", "Comportamiento de la ventana"),
        "Choose what happens when you close the StageSwap window." => ("Choisissez ce qui se passe lorsque vous fermez la fenêtre StageSwap.", "Elige qué ocurre al cerrar la ventana de StageSwap."),
        "Keep running in system tray" => ("Continuer dans la zone de notification", "Seguir en el área de notificación"),
        "Hide the window while StageSwap keeps running." => ("Masquer la fenêtre tout en laissant StageSwap actif.", "Ocultar la ventana mientras StageSwap sigue en ejecución."),
        "Confirm before exit" => ("Confirmer avant de quitter", "Confirmar antes de salir"),
        "Ask before StageSwap fully exits." => ("Demander confirmation avant de quitter complètement StageSwap.", "Preguntar antes de cerrar StageSwap por completo."),
        "Notifications" => ("Notifications", "Notificaciones"),
        "Choose whether StageSwap alerts you when something needs attention." => ("Choisissez si StageSwap vous avertit lorsqu’un élément nécessite votre attention.", "Elige si StageSwap te avisa cuando algo requiere atención."),
        "Show status notifications" => ("Afficher les notifications d’état", "Mostrar notificaciones de estado"),
        "Notify when a component needs attention." => ("Notifier lorsqu’un composant nécessite votre attention.", "Avisar cuando un componente requiera atención."),
        "Choose whether StageSwap shows critical alerts in the app." => ("Choisissez si StageSwap affiche les alertes critiques dans l’application.", "Elige si StageSwap muestra alertas críticas en la aplicación."),
        "Show in-app notifications" => ("Afficher les notifications dans l’application", "Mostrar notificaciones en la aplicación"),
        "Show critical issues in the bell and as brief in-app banners." => ("Afficher les problèmes critiques dans la cloche et sous forme de brèves bannières dans l’application.", "Mostrar problemas críticos en la campana y como avisos breves en la aplicación."),

        // Updates.
        "Updates" => ("Mises à jour", "Actualizaciones"),
        "Check for new StageSwap versions and choose when to install them." => ("Recherchez de nouvelles versions de StageSwap et choisissez quand les installer.", "Busca nuevas versiones de StageSwap y elige cuándo instalarlas."),
        "Current version" => ("Version actuelle", "Versión actual"),
        "Update settings" => ("Paramètres de mise à jour", "Ajustes de actualización"),
        "Choose the update channel and notification preferences." => ("Choisissez le canal de mise à jour et les préférences de notification.", "Elige el canal de actualización y las preferencias de notificación."),
        "Update channel" => ("Canal de mise à jour", "Canal de actualización"),
        "Stable releases are recommended. Beta may include unfinished changes." => ("Les versions stables sont recommandées. Les versions bêta peuvent inclure des modifications inachevées.", "Se recomiendan las versiones estables. Beta puede incluir cambios sin terminar."),
        "Stable releases" => ("Versions stables", "Versiones estables"),
        "Beta" => ("Bêta", "Beta"),
        "Notify when updates are available" => ("Notifier lorsque des mises à jour sont disponibles", "Avisar cuando haya actualizaciones disponibles"),
        "Show one notification for each new version on the selected channel." => ("Afficher une notification pour chaque nouvelle version du canal sélectionné.", "Mostrar una notificación por cada versión nueva del canal seleccionado."),
        "Waiting for an update check." => ("En attente d’une vérification des mises à jour.", "Esperando una comprobación de actualizaciones."),
        "Checking for updates…" => ("Recherche de mises à jour…", "Buscando actualizaciones…"),
        "StageSwap is up to date." => ("StageSwap est à jour.", "StageSwap está actualizado."),
        "StageSwap {0} is available on the selected channel." => ("StageSwap {0} est disponible sur le canal sélectionné.", "StageSwap {0} está disponible en el canal seleccionado."),
        "StageSwap {0} Beta is available on the selected channel." => ("StageSwap {0} Bêta est disponible sur le canal sélectionné.", "StageSwap {0} Beta está disponible en el canal seleccionado."),
        "Downloading and verifying StageSwap {0}…" => ("Téléchargement et vérification de StageSwap {0}…", "Descargando y verificando StageSwap {0}…"),
        "The verified update is starting. StageSwap will restart shortly…" => ("La mise à jour vérifiée démarre. StageSwap va redémarrer sous peu…", "La actualización verificada se está iniciando. StageSwap se reiniciará en breve…"),
        "Update failed: {0}" => ("Échec de la mise à jour : {0}", "Error de actualización: {0}"),
        "Current version: {0}" => ("Version actuelle : {0}", "Versión actual: {0}"),
        "Select a release. Installation starts only after you confirm below." => ("Sélectionnez une version, puis confirmez son installation ci-dessous.", "Selecciona una versión y confirma su instalación abajo."),
        "Retry" => ("Réessayer", "Reintentar"),
        "Details" => ("Détails", "Detalles"),
        "Loading versions…" => ("Chargement des versions…", "Cargando versiones…"),
        "Could not load versions." => ("Impossible de charger les versions.", "No se pudieron cargar las versiones."),
        "Installed" => ("Installée", "Instalada"),
        "StageSwap {0} will be installed and the app will restart." => ("StageSwap {0} sera installé et l’application redémarrera.", "Se instalará StageSwap {0} y la aplicación se reiniciará."),
        "Choose a version to enable installation." => ("Choisissez une version pour activer l’installation.", "Elige una versión para activar la instalación."),
        "Install this version" => ("Installer cette version", "Instalar esta versión"),
        "Install a specific version" => ("Installer une version spécifique", "Instalar una versión específica"),
        "Select version" => ("Choisir une version", "Seleccionar versión"),
        "Choose a previous version to roll back, then install it." => ("Choisissez une version précédente, puis installez-la.", "Elige una versión anterior y luego instálala."),
        "No installable versions found." => ("Aucune version installable trouvée.", "No se encontraron versiones instalables."),
        "Check for updates" => ("Rechercher des mises à jour", "Buscar actualizaciones"),
        "Install update" => ("Installer la mise à jour", "Instalar actualización"),
        "StageSwap update available" => ("Mise à jour StageSwap disponible", "Actualización de StageSwap disponible"),
        "StageSwap {0} is ready. Open Settings → Updates to install it." => ("StageSwap {0} est prêt. Ouvrez Paramètres → Mises à jour pour l’installer.", "StageSwap {0} está listo. Abre Ajustes → Actualizaciones para instalarlo."),
        "Could not save update notification state." => ("Impossible d’enregistrer l’état des notifications de mise à jour.", "No se pudo guardar el estado de las notificaciones de actualización."),
        "You’re all caught up" => ("Vous êtes à jour", "Estás al día"),
        "No recent notifications." => ("Aucune notification récente.", "No hay notificaciones recientes."),
        "Clear all" => ("Tout effacer", "Borrar todo"),
        "Just now" => ("À l’instant", "Ahora mismo"),
        "{0} minute ago" => ("Il y a {0} minute", "Hace {0} minuto"),
        "{0} minutes ago" => ("Il y a {0} minutes", "Hace {0} minutos"),
        "{0} hour ago" => ("Il y a {0} heure", "Hace {0} hora"),
        "{0} hours ago" => ("Il y a {0} heures", "Hace {0} horas"),
        "Startup needs attention" => ("Le démarrage nécessite votre attention", "El inicio requiere atención"),
        "Configuration needs attention" => ("La configuration nécessite votre attention", "La configuración requiere atención"),
        "Device services need attention" => ("Les services vidéo nécessitent votre attention", "Los servicios de vídeo requieren atención"),
        "Frame publisher needs attention" => ("Le diffuseur d’images nécessite votre attention", "El publicador de imágenes requiere atención"),
        "Zoom output needs attention" => ("La sortie Zoom nécessite votre attention", "La salida de Zoom requiere atención"),
        "Webcam needs attention" => ("La webcam nécessite votre attention", "La cámara web requiere atención"),
        "Screen capture needs attention" => ("La capture d’écran nécessite votre attention", "La captura de pantalla requiere atención"),
        "Matching needs attention" => ("La détection nécessite votre attention", "La coincidencia requiere atención"),
        "Reference image needs attention" => ("L’image de référence nécessite votre attention", "La imagen de referencia requiere atención"),
        "Update available" => ("Mise à jour disponible", "Actualización disponible"),
        "Preview activity" => ("Activité de l’aperçu", "Actividad de la vista previa"),
        "Device services" => ("Services vidéo", "Servicios de vídeo"),
        "Frame publisher" => ("Diffuseur d’images", "Publicador de imágenes"),
        "Matching" => ("Détection", "Coincidencia"),
        "Command" => ("Commande", "Comando"),
        "Preview" => ("Aperçu", "Vista previa"),
        "Preview activity notification {0}: 30 seconds have passed." => ("Notification d’activité de l’aperçu {0} : 30 secondes se sont écoulées.", "Notificación de actividad de la vista previa {0}: han pasado 30 segundos."),
        "Language" => ("Langue", "Idioma"),
        "Choose the language used by StageSwap." => ("Choisissez la langue utilisée par StageSwap.", "Elige el idioma que utiliza StageSwap."),
        "Interface language" => ("Langue de l’interface", "Idioma de la interfaz"),
        "Changes apply immediately." => ("Les modifications s’appliquent immédiatement.", "Los cambios se aplican de inmediato."),

        // Webcam and screen settings.
        "No camera selected" => ("Aucune caméra sélectionnée", "No hay ninguna cámara seleccionada"),
        "Saved camera is unavailable" => ("La caméra enregistrée est indisponible", "La cámara guardada no está disponible"),
        "Selected webcam" => ("Webcam sélectionnée", "Cámara web seleccionada"),
        "No webcam frame — choose a camera or refresh the device list." => ("Aucune image de webcam — choisissez une caméra ou actualisez la liste des appareils.", "No hay imagen de la cámara web: elige una cámara o actualiza la lista de dispositivos."),
        "Refresh camera devices" => ("Actualiser les caméras", "Actualizar cámaras"),
        "Crop webcam to 16:9" => ("Recadrer la webcam en 16:9", "Recortar la cámara web a 16:9"),
        "Crop non-16:9 cameras to fill the frame." => ("Recadrer les caméras qui ne sont pas en 16:9 afin de remplir l’image.", "Recortar las cámaras que no sean 16:9 para llenar el encuadre."),
        "No display selected" => ("Aucun écran sélectionné", "No hay ninguna pantalla seleccionada"),
        "Live secondary screen" => ("Écran secondaire en direct", "Pantalla secundaria en directo"),
        "No secondary screen image — choose a screen or use Tools in Diagnostics." => ("Aucune image de l’écran secondaire — choisissez un écran ou utilisez Outils dans Diagnostic.", "No hay imagen de la pantalla secundaria: elige una pantalla o usa Herramientas en Diagnóstico."),
        "Capture behavior" => ("Comportement de la capture", "Comportamiento de la captura"),
        "Include mouse cursor" => ("Inclure le pointeur de la souris", "Incluir el cursor"),
        "New reference images use this setting; existing and imported images do not change." => ("Les nouvelles images de référence utilisent ce réglage ; les images existantes et importées ne changent pas.", "Las imágenes de referencia nuevas usan este ajuste; las imágenes existentes e importadas no cambian."),
        "Automatic screen tools" => ("Outils automatiques pour l’écran", "Herramientas automáticas de pantalla"),
        "Find the JW Library screen automatically" => ("Trouver automatiquement l’écran utilisé par JW Library", "Buscar automáticamente la pantalla que usa JW Library"),
        "Automatically fix screen capture problems" => ("Résoudre automatiquement les problèmes de capture d’écran", "Solucionar automáticamente los problemas de captura de pantalla"),
        "Looks for the screen JW Library uses when StageSwap starts, when Settings opens, after the reference image changes, and every 30 seconds. It confirms the same screen twice before selecting it." => ("Recherche l’écran utilisé par JW Library au démarrage de StageSwap, à l’ouverture des Paramètres, après chaque changement de l’image de référence et toutes les 30 secondes. StageSwap confirme deux fois le même écran avant de le sélectionner.", "Busca la pantalla que usa JW Library al iniciar StageSwap, al abrir Ajustes, después de cambiar la imagen de referencia y cada 30 segundos. StageSwap confirma dos veces la misma pantalla antes de seleccionarla."),
        "Checks the selected screen every 30 seconds. If the image is black or missing twice in a row, StageSwap restarts screen capture." => ("Vérifie l’écran sélectionné toutes les 30 secondes. Si l’image est noire ou absente deux fois de suite, StageSwap redémarre la capture d’écran.", "Comprueba la pantalla seleccionada cada 30 segundos. Si la imagen está en negro o falta dos veces seguidas, StageSwap reinicia la captura de pantalla."),

        // Reference image and media detection.
        "Reference image" => ("Image de référence", "Imagen de referencia"),
        "No reference image — show the screen JW Library shows when no media is playing, then capture it." => ("Aucune image de référence — affichez l’écran présenté par JW Library lorsqu’aucun média n’est en cours de lecture, puis capturez-le.", "No hay imagen de referencia: muestra la pantalla que presenta JW Library cuando no se reproduce contenido multimedia y captúrala."),
        "Checks 4×/s · confirms after 5 matches or 3 differences · 0.5s fade" => ("4 vérifications/s · confirmation après 5 correspondances ou 3 différences · fondu de 0,5 s", "4 comprobaciones/s · confirma tras 5 coincidencias o 3 diferencias · fundido de 0,5 s"),
        "Capture reference image" => ("Capturer l’image de référence", "Capturar imagen de referencia"),
        "Import image…" => ("Importer une image…", "Importar imagen…"),
        "Required similarity" => ("Similarité requise", "Similitud requerida"),
        "Reset 98%" => ("Réinitialiser à 98 %", "Restablecer al 98 %"),
        "Very high" => ("Très élevée", "Muy alta"),
        "High" => ("Élevée", "Alta"),
        "Moderate" => ("Moyenne", "Media"),
        "Low" => ("Faible", "Baja"),
        "Small visual changes may count as media." => ("De petits changements visuels peuvent être considérés comme un média.", "Los pequeños cambios visuales pueden contar como contenido multimedia."),
        "Minor rendering or cursor differences are ignored." => ("Les légères différences de rendu ou de pointeur sont ignorées.", "Se ignoran las pequeñas diferencias de renderizado o del cursor."),
        "Larger visual differences may still count as no media." => ("Des différences visuelles plus importantes peuvent encore être considérées comme une absence de média.", "Las diferencias visuales más grandes todavía pueden contar como ausencia de contenido multimedia."),
        "Significant changes may still count as no media." => ("Des changements importants peuvent encore être considérés comme une absence de média.", "Los cambios importantes todavía pueden contar como ausencia de contenido multimedia."),
        "Still-image picture-in-picture" => ("Image fixe en incrustation", "Imagen fija en imagen superpuesta"),
        "Choose the PIP layout and optionally use it automatically for still images." => ("Choisissez la disposition de l’incrustation et utilisez-la éventuellement de façon automatique pour les images fixes.", "Elige el diseño de la imagen superpuesta y, si quieres, úsalo automáticamente para imágenes fijas."),
        "Keep the speaker visible when a presentation image stays unchanged." => ("Gardez l’orateur visible lorsqu’une image de présentation reste inchangée.", "Mantén visible al orador cuando una imagen de la presentación permanece sin cambios."),
        "Use picture-in-picture for still images" => ("Utiliser l’incrustation pour les images fixes", "Usar imagen superpuesta para imágenes fijas"),
        "Use automatically for still images" => ("Utiliser automatiquement pour les images fixes", "Usar automáticamente para imágenes fijas"),
        "In Auto mode, show both feeds after a non-reference image remains unchanged." => ("En mode Auto, affiche les deux sources lorsqu’une image différente de la référence reste inchangée.", "En modo Auto, muestra ambas fuentes cuando una imagen distinta de la referencia permanece sin cambios."),
        "Show picture-in-picture after" => ("Afficher l’incrustation après", "Mostrar la imagen superpuesta después de"),
        "Any screen movement restarts this timer." => ("Tout mouvement à l’écran redémarre ce délai.", "Cualquier movimiento en la pantalla reinicia este tiempo."),
        "30 seconds" => ("30 secondes", "30 segundos"),
        "45 seconds" => ("45 secondes", "45 segundos"),
        "1 minute" => ("1 min", "1 minuto"),
        "2 minutes" => ("2 min", "2 minutos"),
        "5 minutes" => ("5 min", "5 minutos"),
        "Main view" => ("Vue principale", "Vista principal"),
        "The other live feed appears in the bottom-left inset." => ("L’autre source en direct apparaît dans l’incrustation en bas à gauche.", "La otra fuente en directo aparece superpuesta en la esquina inferior izquierda."),
        "Used by automatic and forced PIP; the other feed appears in the bottom-left inset." => ("Utilisée pour l’incrustation automatique et forcée ; l’autre source apparaît en bas à gauche.", "Se usa para la imagen superpuesta automática y forzada; la otra fuente aparece en la esquina inferior izquierda."),
        "Webcam full screen" => ("Webcam en plein écran", "Cámara web a pantalla completa"),
        "Secondary screen full screen" => ("Écran secondaire en plein écran", "Pantalla secundaria a pantalla completa"),
        "PIP size" => ("Taille de l’incrustation", "Tamaño de la imagen superpuesta"),
        "Choose how much of the main view the inset covers." => ("Choisissez la portion de la vue principale couverte par l’incrustation.", "Elige cuánto espacio de la vista principal ocupa la imagen superpuesta."),
        "Mini" => ("Petite", "Pequeña"),
        "Medium" => ("Moyenne", "Mediana"),
        "Large" => ("Grande", "Grande"),

        // Diagnostics.
        "Component health" => ("État des composants", "Estado de los componentes"),
        "Check whether each video component and media detection are working." => ("Vérifiez que chaque composant vidéo et la détection des médias fonctionnent.", "Comprueba si cada componente de vídeo y la detección de contenido multimedia funcionan."),
        "Virtual camera" => ("Caméra virtuelle", "Cámara virtual"),
        "Screen capture" => ("Capture d’écran", "Captura de pantalla"),
        "Tools" => ("Outils", "Herramientas"),
        "Rescan for the JW Library screen or restart a video component." => ("Recherchez à nouveau l’écran JW Library ou redémarrez un composant vidéo.", "Vuelve a buscar la pantalla de JW Library o reinicia un componente de vídeo."),
        "Rescan displays" => ("Rechercher les écrans", "Buscar pantallas"),
        "Restart webcam" => ("Redémarrer la webcam", "Reiniciar cámara web"),
        "Restart screen capture" => ("Redémarrer la capture d’écran", "Reiniciar captura de pantalla"),
        "Restart virtual camera" => ("Redémarrer la caméra virtuelle", "Reiniciar cámara virtual"),
        "Restart all" => ("Tout redémarrer", "Reiniciar todo"),
        "Recovery" => ("Récupération", "Recuperación"),
        "Open reference capture" => ("Ouvrir la capture de référence", "Abrir captura de referencia"),
        "Open camera privacy settings" => ("Ouvrir les paramètres de confidentialité de la caméra", "Abrir la configuración de privacidad de la cámara"),
        "Technical details" => ("Détails techniques", "Detalles técnicos"),
        "View the devices, formats, and timing StageSwap is currently using." => ("Consultez les appareils, les formats et la temporisation actuellement utilisés par StageSwap.", "Consulta los dispositivos, formatos y tiempos que StageSwap utiliza actualmente."),
        "Webcam device ID" => ("Identifiant de la webcam", "ID de la cámara web"),
        "Webcam format" => ("Format de la webcam", "Formato de la cámara web"),
        "Video output" => ("Sortie vidéo", "Salida de video"),
        "Transitions" => ("Transitions", "Transiciones"),
        "Detection timing" => ("Temporisation de la détection", "Frecuencia de detección"),
        "Storage and logs" => ("Stockage et journaux", "Almacenamiento y registros"),
        "Find saved settings and logs, or export logs for troubleshooting." => ("Retrouvez les paramètres et les journaux enregistrés, ou exportez les journaux pour résoudre un problème.", "Busca los ajustes y registros guardados o exporta los registros para solucionar problemas."),
        "Configuration" => ("Configuration", "Configuración"),
        "Log directory" => ("Dossier des journaux", "Carpeta de registros"),
        "Diagnostic logs" => ("Journaux de diagnostic", "Registros de diagnóstico"),
        "Stored locally for troubleshooting." => ("Stockés localement pour le dépannage.", "Se guardan localmente para solucionar problemas."),
        "Logs are retained for 14 days." => ("Les journaux sont conservés pendant 14 jours.", "Los registros se conservan durante 14 días."),
        "Verbose diagnostic logging" => ("Journalisation de diagnostic détaillée", "Registro de diagnóstico detallado"),
        "Include detailed runtime activity and periodic health checks in local logs. Enable this only while troubleshooting." => ("Inclut l’activité détaillée du runtime et les contrôles de santé périodiques dans les journaux locaux. Activez cette option uniquement pour résoudre un problème.", "Incluye la actividad detallada del tiempo de ejecución y las comprobaciones periódicas del estado en los registros locales. Actívalo solo para solucionar problemas."),
        "Open log folder" => ("Ouvrir le dossier des journaux", "Abrir carpeta de registros"),
        "Export logs…" => ("Exporter les journaux…", "Exportar registros…"),
        "Clear logs…" => ("Effacer les journaux…", "Borrar registros…"),
        "Open folder" => ("Ouvrir le dossier", "Abrir carpeta"),
        "Export…" => ("Exporter…", "Exportar…"),
        "Clear…" => ("Effacer…", "Borrar…"),
        "The webcam needs attention. Choose or refresh the camera in Webcam, then restart it here if needed." => ("La webcam nécessite votre attention. Choisissez ou actualisez la caméra dans Webcam, puis redémarrez-la ici si nécessaire.", "La cámara web requiere atención. Elige o actualiza la cámara en Cámara web y reiníciala aquí si es necesario."),
        "Screen capture needs attention. Choose a display in Screen, then restart capture here if needed." => ("La capture d’écran nécessite votre attention. Choisissez un écran dans Écran, puis redémarrez la capture ici si nécessaire.", "La captura de pantalla requiere atención. Elige una pantalla en Pantalla y reinicia la captura aquí si es necesario."),
        "The virtual camera needs attention. Restart it here, then reselect Stageswap Camera in Zoom if necessary." => ("La caméra virtuelle nécessite votre attention. Redémarrez-la ici, puis sélectionnez de nouveau Stageswap Camera dans Zoom si nécessaire.", "La cámara virtual requiere atención. Reiníciala aquí y vuelve a seleccionar Stageswap Camera en Zoom si es necesario."),
        "One or more components are still starting. Wait briefly before using a tool." => ("Un ou plusieurs composants sont encore en cours de démarrage. Patientez un instant avant d’utiliser un outil.", "Uno o varios componentes todavía se están iniciando. Espera un momento antes de usar una herramienta."),
        "The video components are ready, but Auto mode needs a captured or imported reference image." => ("Les composants vidéo sont prêts, mais le mode Auto nécessite une image de référence capturée ou importée.", "Los componentes de video están listos, pero el modo Auto necesita una imagen de referencia capturada o importada."),
        "The components are ready; StageSwap is checking the reference image." => ("Les composants sont prêts ; StageSwap vérifie l’image de référence.", "Los componentes están listos; StageSwap está comprobando la imagen de referencia."),
        "Everything is ready." => ("Tout est prêt.", "Todo está listo."),
        "Admin configuration" => ("Configuration administrateur", "Configuración de administrador"),

        // Dialogs.
        "Exit StageSwap?" => ("Quitter StageSwap ?", "¿Salir de StageSwap?"),
        "Clear diagnostic logs?" => ("Effacer les journaux de diagnostic ?", "¿Borrar los registros de diagnóstico?"),
        "Replace saved configuration?" => ("Remplacer la configuration enregistrée ?", "¿Sustituir la configuración guardada?"),
        "Load saved configuration?" => ("Charger la configuration enregistrée ?", "¿Cargar la configuración guardada?"),
        "Delete saved configuration?" => ("Supprimer la configuration enregistrée ?", "¿Eliminar la configuración guardada?"),
        "StageSwap will stop publishing. The virtual camera stays installed and shows the StageSwap off screen until the app starts again." => ("StageSwap cessera de diffuser. La caméra virtuelle reste installée et affiche l’écran d’arrêt de StageSwap jusqu’au prochain démarrage de l’application.", "StageSwap dejará de emitir. La cámara virtual seguirá instalada y mostrará la pantalla de StageSwap desactivado hasta que la aplicación vuelva a iniciarse."),
        "This permanently removes locally stored diagnostic logs. New logs will continue to be recorded." => ("Cette action supprime définitivement les journaux de diagnostic stockés localement. De nouveaux journaux continueront d’être enregistrés.", "Esto elimina de forma permanente los registros de diagnóstico guardados localmente. Se seguirán registrando nuevos datos."),
        "Replace the saved admin config with the setup currently shown in Settings?" => ("Remplacer la configuration administrateur enregistrée par celle actuellement affichée dans Paramètres ?", "¿Sustituir la configuración de administrador guardada por la que se muestra actualmente en Ajustes?"),
        "Replace the current settings and reference image with the saved admin config? Current session changes will be lost." => ("Remplacer les paramètres actuels et l’image de référence par la configuration administrateur enregistrée ? Les modifications de la session seront perdues.", "¿Sustituir los ajustes actuales y la imagen de referencia por la configuración de administrador guardada? Se perderán los cambios de la sesión."),
        "Auto-restore will turn off. Your current settings and reference image will stay unchanged." => ("La restauration automatique sera désactivée. Vos paramètres et votre image de référence actuels resteront inchangés.", "La restauración automática se desactivará. Los ajustes actuales y la imagen de referencia no cambiarán."),
        "Stay open" => ("Rester ouvert", "Mantener abierto"),
        "Exit StageSwap" => ("Quitter StageSwap", "Salir de StageSwap"),
        "Keep logs" => ("Conserver les journaux", "Conservar registros"),
        "Clear logs" => ("Effacer les journaux", "Borrar registros"),
        "Keep saved configuration" => ("Conserver la configuration enregistrée", "Conservar configuración guardada"),
        "Save current configuration" => ("Enregistrer la configuration actuelle", "Guardar configuración actual"),
        "Keep current config" => ("Conserver la configuration actuelle", "Conservar configuración actual"),
        "Load saved configuration" => ("Charger la configuration enregistrée", "Cargar configuración guardada"),
        "Delete saved configuration" => ("Supprimer la configuration enregistrée", "Eliminar configuración guardada"),
        "Keep a protected local copy of the current settings and reference image for managed setups." => ("Conservez une copie locale protégée des paramètres actuels et de l’image de référence pour les installations gérées.", "Conserva una copia local protegida de los ajustes actuales y la imagen de referencia para instalaciones administradas."),
        "No admin config is saved." => ("Aucune configuration administrateur n’est enregistrée.", "No hay ninguna configuración de administrador guardada."),
        "Settings and reference image saved in the admin config" => ("Paramètres et image de référence enregistrés dans la configuration administrateur", "Ajustes e imagen de referencia guardados en la configuración de administrador"),
        "Admin config saved without a reference image" => ("Configuration administrateur enregistrée sans image de référence", "Configuración de administrador guardada sin imagen de referencia"),
        "Auto-restore on launch" => ("Restaurer automatiquement au démarrage", "Restaurar automáticamente al iniciar"),
        "Replace session changes with this admin config whenever StageSwap starts." => ("Remplacer les modifications de la session par cette configuration administrateur à chaque démarrage de StageSwap.", "Sustituir los cambios de la sesión por esta configuración de administrador cada vez que se inicie StageSwap."),

        // Full-page guided setup.
        "Step {0} of {1}" => ("Étape {0} sur {1}", "Paso {0} de {1}"),
        "Set up later" => ("Configurer plus tard", "Configurar más tarde"),
        "Start StageSwap" => ("Démarrer StageSwap", "Iniciar StageSwap"),
        "JW Library to Zoom" => ("JW Library vers Zoom", "De JW Library a Zoom"),
        "Automatically switch what Zoom sees between the webcam and JW Library presentations." => ("Alternez automatiquement ce que Zoom voit entre la webcam et les présentations JW Library.", "Cambia automáticamente lo que ve Zoom entre la cámara web y las presentaciones de JW Library."),
        "No media in JW Library → Zoom sees the webcam" => ("Aucun média affiché dans JW Library → Zoom voit la webcam", "Sin contenido multimedia en JW Library → Zoom ve la cámara web"),
        "Media detected in JW Library → Zoom sees the secondary screen" => ("Média détecté dans JW Library → Zoom voit l’écran secondaire", "Contenido multimedia detectado en JW Library → Zoom ve la pantalla secundaria"),
        "StageSwap sends the webcam or JW Library screen to Zoom through one virtual camera." => ("StageSwap transmet la webcam ou l’écran JW Library à Zoom via une seule caméra virtuelle.", "StageSwap envía la cámara web o la pantalla de JW Library a Zoom mediante una única cámara virtual."),
        "Choose your webcam" => ("Choisissez votre webcam", "Elige tu cámara web"),
        "Choose the webcam Zoom should see when JW Library is not playing media." => ("Choisissez la webcam que Zoom doit voir lorsqu’aucun média n’est affiché dans JW Library.", "Elige la cámara web que Zoom debe ver cuando JW Library no reproduzca contenido multimedia."),
        "Webcam preview" => ("Aperçu de la webcam", "Vista previa de la cámara web"),
        "No webcam found. Connect a webcam, then refresh the list." => ("Aucune webcam détectée. Connectez une webcam, puis actualisez la liste.", "No se encontró ninguna cámara web. Conecta una y actualiza la lista."),
        "This webcam is unavailable. Choose another one or refresh the list." => ("Cette webcam est indisponible. Choisissez-en une autre ou actualisez la liste.", "Esta cámara web no está disponible. Elige otra o actualiza la lista."),
        "Refresh webcams" => ("Actualiser les webcams", "Actualizar cámaras web"),
        "Choose the secondary screen" => ("Choisissez l’écran secondaire", "Elige la pantalla secundaria"),
        "Secondary screen preview" => ("Aperçu de l’écran secondaire", "Vista previa de la pantalla secundaria"),
        "No screen found. Connect the secondary screen used by JW Library, then rescan." => ("Aucun écran détecté. Connectez l’écran secondaire utilisé par JW Library, puis relancez la recherche.", "No se encontró ninguna pantalla. Conecta la pantalla secundaria que usa JW Library y vuelve a buscar."),
        "This secondary screen is unavailable. Choose another one or rescan." => ("Cet écran secondaire est indisponible. Choisissez-en un autre ou relancez la recherche.", "Esta pantalla secundaria no está disponible. Elige otra o vuelve a buscar."),
        "Capture the screen JW Library shows when no media is playing as the reference image." => ("Capturez comme image de référence l’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture.", "Captura como imagen de referencia la pantalla que muestra JW Library cuando no se reproduce contenido multimedia."),
        "Set the reference image StageSwap uses to know when to show the JW Library screen in Zoom." => ("Définissez l’image de référence que StageSwap utilise pour savoir quand afficher l’écran JW Library dans Zoom.", "Define la imagen de referencia que StageSwap usa para saber cuándo mostrar la pantalla de JW Library en Zoom."),
        "Show the screen JW Library shows when no media is playing, then capture the live image below." => ("Affichez l’écran présenté par JW Library lorsqu’aucun média n’est en cours de lecture, puis capturez l’image en direct ci-dessous.", "Muestra la pantalla que presenta JW Library cuando no se reproduce contenido multimedia y captura la imagen en directo que aparece abajo."),
        "Example with no media" => ("Exemple sans média", "Ejemplo sin contenido multimedia"),
        "The screen JW Library shows when no media is playing has centered text and a gray square in the corner; this example uses unbranded shapes." => ("L’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture comporte du texte centré et un carré gris dans un coin ; cet exemple utilise des formes sans marque.", "La pantalla que muestra JW Library cuando no se reproduce contenido multimedia tiene texto centrado y un cuadrado gris en una esquina; este ejemplo usa formas sin marca."),
        "Prepare JW Library" => ("Préparez JW Library", "Prepara JW Library"),
        "Show the screen JW Library shows when no media is playing. It has centered text and a gray square in the corner." => ("Affichez l’écran présenté par JW Library lorsqu’aucun média n’est en cours de lecture. Il comporte du texte centré et un carré gris dans un coin.", "Muestra la pantalla que presenta JW Library cuando no se reproduce contenido multimedia. Tiene texto centrado y un cuadrado gris en una esquina."),
        "Change display" => ("Changer d’écran", "Cambiar pantalla"),
        "Choose a display" => ("Choisir un écran", "Elegir una pantalla"),
        "What to capture" => ("Ce qu’il faut capturer", "Qué capturar"),
        "Choose the secondary screen before capturing a reference image." => ("Choisissez l’écran secondaire avant de capturer une image de référence.", "Elige la pantalla secundaria antes de capturar una imagen de referencia."),
        "Check the live preview, then capture the screen JW Library shows when no media is playing as the reference image." => ("Vérifiez l’aperçu en direct, puis capturez comme image de référence l’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture.", "Comprueba la vista previa en directo y captura como imagen de referencia la pantalla que muestra JW Library cuando no se reproduce contenido multimedia."),
        "No reference image captured yet." => ("Aucune image de référence capturée pour le moment.", "Aún no se ha capturado ninguna imagen de referencia."),
        "Reference image captured" => ("Image de référence capturée", "Imagen de referencia capturada"),
        "Capturing the current frame…" => ("Capture de l’image actuelle…", "Capturando la imagen actual…"),
        "No live display frame yet. Check the connection or choose another display." => ("Aucune image d’écran en direct pour le moment. Vérifiez la connexion ou choisissez un autre écran.", "Aún no hay imagen de pantalla en directo. Comprueba la conexión o elige otra pantalla."),
        "StageSwap will use this image to detect when JW Library is not playing media." => ("StageSwap utilisera cette image pour détecter lorsqu’aucun média n’est affiché dans JW Library.", "StageSwap usará esta imagen para detectar cuándo JW Library no reproduce contenido multimedia."),
        "Saved reference image" => ("Image de référence enregistrée", "Imagen de referencia guardada"),
        "Your captured image" => ("Votre image capturée", "Tu imagen capturada"),
        "CONFIRMED" => ("CONFIRMÉE", "CONFIRMADA"),
        "TO CONFIRM" => ("À CONFIRMER", "POR CONFIRMAR"),
        "No captured image available for review." => ("Aucune image capturée disponible pour vérification.", "No hay ninguna imagen capturada disponible para revisar."),
        "Saving reference…" => ("Enregistrement de la référence…", "Guardando referencia…"),
        "StageSwap couldn’t save this reference. Try again or retake the image." => ("StageSwap n’a pas pu enregistrer cette référence. Réessayez ou reprenez la capture.", "StageSwap no pudo guardar esta referencia. Vuelve a intentarlo o repite la captura."),
        "Retake" => ("Reprendre", "Repetir captura"),
        "Try again" => ("Réessayer", "Reintentar"),
        "Use this image" => ("Utiliser cette image", "Usar esta imagen"),
        "Confirm reference image" => ("Confirmer l’image de référence", "Confirmar imagen de referencia"),
        "Make sure this image is the screen JW Library shows when no media is playing." => ("Vérifiez que cette image correspond à l’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture.", "Comprueba que esta imagen sea la pantalla que muestra JW Library cuando no se reproduce contenido multimedia."),
        "Example reference image" => ("Exemple d’image de référence", "Ejemplo de imagen de referencia"),
        "The screen JW Library shows when no media is playing should look like this example." => ("L’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture doit ressembler à cet exemple.", "La pantalla que muestra JW Library cuando no se reproduce contenido multimedia debe parecerse a este ejemplo."),
        "Capture again" => ("Capturer à nouveau", "Capturar de nuevo"),
        "StageSwap couldn’t capture the screen. Check the screen preview and try again." => ("StageSwap n’a pas pu capturer l’écran. Vérifiez l’aperçu de l’écran et réessayez.", "StageSwap no pudo capturar la pantalla. Comprueba la vista previa de la pantalla e inténtalo de nuevo."),
        "Ready for the meeting" => ("Prêt pour la réunion", "Todo listo para la reunión"),
        "Review the setup status below, then start automatic switching." => ("Vérifiez l’état de la configuration ci-dessous, puis activez le changement automatique.", "Revisa el estado de la configuración y activa el cambio automático."),
        "Webcam ready" => ("Webcam prête", "Cámara web lista"),
        "Webcam not selected" => ("Webcam non sélectionnée", "Cámara web no seleccionada"),
        "Secondary screen ready" => ("Écran secondaire prêt", "Pantalla secundaria lista"),
        "Secondary screen not selected" => ("Écran secondaire non sélectionné", "Pantalla secundaria no seleccionada"),
        "Reference image ready" => ("Image de référence prête", "Imagen de referencia lista"),
        "Reference image not captured" => ("Image de référence non capturée", "Imagen de referencia no capturada"),
        "IMPORTANT" => ("ATTENTION", "IMPORTANTE"),
        "In Zoom, select Stageswap Camera as your camera before the meeting." => ("Dans Zoom, sélectionnez Stageswap Camera comme caméra avant la réunion.", "En Zoom, selecciona Stageswap Camera como cámara antes de la reunión."),
        "Some setup is missing. StageSwap will start, but Auto mode may not work as expected. You can finish the guided setup later in Settings." => ("Certains éléments ne sont pas configurés. StageSwap va démarrer, mais le mode Auto risque de ne pas fonctionner comme prévu. Vous pourrez terminer la configuration guidée plus tard dans Paramètres.", "Faltan algunos elementos de la configuración. StageSwap se iniciará, pero es posible que el modo Auto no funcione como esperas. Puedes terminar la configuración guiada más tarde en Ajustes."),
        "Guided setup" => ("Configuration guidée", "Configuración guiada"),
        "Choose the webcam and secondary screen, then capture the screen JW Library shows when no media is playing." => ("Choisissez la webcam et l’écran secondaire, puis capturez l’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture.", "Elige la cámara web y la pantalla secundaria y captura la pantalla que muestra JW Library cuando no se reproduce contenido multimedia."),
        "Open guided setup" => ("Ouvrir la configuration guidée", "Abrir la configuración guiada"),

        // Tray and Windows notifications.
        "Open StageSwap" => ("Ouvrir StageSwap", "Abrir StageSwap"),
        "Webcam only" => ("Webcam uniquement", "Solo cámara web"),
        "Screen only" => ("Écran uniquement", "Solo pantalla"),
        "Picture-in-picture" => ("Incrustation", "Imagen superpuesta"),
        "Exit" => ("Quitter", "Salir"),
        "StageSwap needs attention" => ("StageSwap nécessite votre attention", "StageSwap requiere atención"),
        "StageSwap installation failed" => ("Échec de l’installation de StageSwap", "Error al instalar StageSwap"),
        "StageSwap is already running" => ("StageSwap est déjà en cours d’exécution", "StageSwap ya está en ejecución"),
        "StageSwap could not start" => ("Impossible de démarrer StageSwap", "No se pudo iniciar StageSwap"),
        "StageSwap could not start its local control service" => ("StageSwap n’a pas pu démarrer son service de contrôle local", "StageSwap no pudo iniciar su servicio de control local"),
        "StageSwap deployment failed" => ("Échec du déploiement de StageSwap", "Error en el despliegue de StageSwap"),
        "Import reference image" => ("Importer une image de référence", "Importar imagen de referencia"),
        "Export diagnostic logs" => ("Exporter les journaux de diagnostic", "Exportar registros de diagnóstico"),
        "Image files" => ("Fichiers image", "Archivos de imagen"),
        "PNG files" => ("Fichiers PNG", "Archivos PNG"),
        "JPEG files" => ("Fichiers JPEG", "Archivos JPEG"),
        "BMP files" => ("Fichiers BMP", "Archivos BMP"),
        "JSON Lines files" => ("Fichiers JSON Lines", "Archivos JSON Lines"),
        "All files" => ("Tous les fichiers", "Todos los archivos"),

        // Portable install and update prompts.
        "Replace the installed StageSwap?" => ("Remplacer la version installée de StageSwap ?", "¿Sustituir la versión instalada de StageSwap?"),
        "Update installed StageSwap" => ("Mettre à jour StageSwap", "Actualizar StageSwap instalado"),
        "Open installed StageSwap" => ("Ouvrir StageSwap installé", "Abrir StageSwap instalado"),
        "Install StageSwap for this user?" => ("Installer StageSwap pour cet utilisateur ?", "¿Instalar StageSwap para este usuario?"),
        "Installation keeps the app at a stable per-user path and creates Start Menu and Desktop shortcuts. The virtual-camera component still requires administrator approval." => ("L’installation conserve l’application dans un emplacement stable propre à l’utilisateur et crée des raccourcis dans le menu Démarrer et sur le Bureau. Le composant de caméra virtuelle nécessite toujours l’autorisation d’un administrateur.", "La instalación mantiene la aplicación en una ruta estable para el usuario y crea accesos directos en el menú Inicio y en el Escritorio. El componente de cámara virtual sigue necesitando aprobación de administrador."),
        "Install StageSwap\nRecommended for startup and upgrades" => ("Installer StageSwap\nRecommandé pour le démarrage et les mises à jour", "Instalar StageSwap\nRecomendado para el inicio y las actualizaciones"),
        "Run once\nDo not copy this executable" => ("Exécuter une fois\nNe pas copier cet exécutable", "Ejecutar una vez\nNo copiar este ejecutable"),
        "This replaces the installed app with an older version." => ("Cette opération remplace l’application installée par une version antérieure.", "Esto sustituye la aplicación instalada por una versión anterior."),
        "This replaces it with a different build of the same version." => ("Cette opération la remplace par une autre compilation de la même version.", "Esto la sustituye por otra compilación de la misma versión."),
        "The running app will close gracefully before replacement." => ("L’application en cours d’exécution se fermera correctement avant son remplacement.", "La aplicación en ejecución se cerrará correctamente antes de sustituirse.")
        ,"Installed version: {0}\nCandidate version: {1}\n\n{2}" => ("Version installée : {0}\nVersion candidate : {1}\n\n{2}", "Versión instalada: {0}\nVersión candidata: {1}\n\n{2}")
    }))
}

pub fn format_text(locale: Locale, source: &str, arguments: &[&str]) -> String {
    let mut result = text(locale, source).into_owned();
    for (index, value) in arguments.iter().enumerate() {
        result = result.replace(&format!("{{{index}}}"), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_locale_resolution_normalizes_tags_and_prefers_saved_system_then_english() {
        assert_eq!(Locale::from_tag("fr-CA"), Some(Locale::French));
        assert_eq!(Locale::from_tag("es_MX"), Some(Locale::Spanish));
        assert_eq!(Locale::from_tag("EN-gb"), Some(Locale::English));
        assert_eq!(Locale::from_tag("de-DE"), None);
        assert_eq!(Locale::Spanish.tag(), "es");
        assert_eq!(
            Locale::resolve(Some("fr-FR"), Some("es-ES")),
            Locale::French
        );
        assert_eq!(Locale::resolve(None, Some("es-MX")), Locale::Spanish);
        assert_eq!(
            Locale::resolve(Some("unknown"), Some("fr-CA")),
            Locale::French
        );
        assert_eq!(Locale::resolve(None, Some("de-DE")), Locale::English);
    }

    #[test]
    fn contract_localization_lookup_preserves_unknown_text_and_formats_placeholders() {
        assert_eq!(text(Locale::French, "Settings"), "Paramètres");
        assert_eq!(text(Locale::Spanish, "Settings"), "Ajustes");
        assert_eq!(
            text(Locale::French, "Device supplied name"),
            "Device supplied name"
        );
        assert_eq!(
            format_text(Locale::French, "{0} — {1}", &["98 %", "Équilibrée"]),
            "98 % — Équilibrée"
        );
    }

    #[test]
    fn contract_approved_terminology_and_guided_setup_copy_are_localized_consistently() {
        let terms = [
            (
                "Reference image",
                "Image de référence",
                "Imagen de referencia",
            ),
            (
                "Secondary screen",
                "Écran secondaire",
                "Pantalla secundaria",
            ),
            ("Zoom output", "Sortie Zoom", "Salida de Zoom"),
            (
                "Automatic switching",
                "Changement automatique",
                "Cambio automático",
            ),
            (
                "Start automatic switching",
                "Activer le changement automatique",
                "Activar el cambio automático",
            ),
            ("Auto", "Auto", "Auto"),
            ("Camera", "Caméra", "Cámara"),
            ("Screen", "Écran", "Pantalla"),
            ("Webcam", "Webcam", "Cámara web"),
            ("No media", "Aucun média", "Sin contenido multimedia"),
            (
                "Media detected",
                "Média détecté",
                "Contenido multimedia detectado",
            ),
            (
                "Media detection",
                "Détection des médias",
                "Detección de contenido multimedia",
            ),
            (
                "Capture reference image",
                "Capturer l’image de référence",
                "Capturar imagen de referencia",
            ),
            (
                "Required similarity",
                "Similarité requise",
                "Similitud requerida",
            ),
            ("Output mode", "Mode de sortie", "Modo de salida"),
            ("Tools", "Outils", "Herramientas"),
            ("Virtual camera", "Caméra virtuelle", "Cámara virtual"),
            (
                "System tray",
                "Zone de notification",
                "Área de notificación",
            ),
            (
                "Start automatic switching when StageSwap launches.",
                "Lancer le changement automatique au démarrage de StageSwap.",
                "Iniciar el cambio automático al iniciar StageSwap.",
            ),
            (
                "Guided setup",
                "Configuration guidée",
                "Configuración guiada",
            ),
        ];

        for (english, french, spanish) in terms {
            assert_eq!(text(Locale::English, english), english);
            assert_eq!(text(Locale::French, english), french);
            assert_eq!(text(Locale::Spanish, english), spanish);
        }

        assert_eq!(
            text(Locale::French, "Capture reference image"),
            "Capturer l’image de référence"
        );
        assert_eq!(
            text(
                Locale::Spanish,
                "Media detected in JW Library → Zoom sees the secondary screen"
            ),
            "Contenido multimedia detectado en JW Library → Zoom ve la pantalla secundaria"
        );
        assert_eq!(
            text(
                Locale::French,
                "Media detected in JW Library → Zoom sees the secondary screen"
            ),
            "Média détecté dans JW Library → Zoom voit l’écran secondaire"
        );
        assert_eq!(
            text(Locale::French, "Open guided setup"),
            "Ouvrir la configuration guidée"
        );
        assert_eq!(
            text(Locale::Spanish, "Ready for the meeting"),
            "Todo listo para la reunión"
        );
        assert_eq!(
            format_text(Locale::French, "Step {0} of {1}", &["4", "5"]),
            "Étape 4 sur 5"
        );
        assert_eq!(
            format_text(Locale::Spanish, "Step {0} of {1}", &["4", "5"]),
            "Paso 4 de 5"
        );
    }

    #[test]
    fn contract_jw_library_copy_is_localized_in_every_supported_language() {
        let strings = [
            "Automatically switch what Zoom sees between the webcam and JW Library presentations.",
            "Choose how StageSwap starts, stays open, and alerts you.",
            "Choose the webcam Zoom sees when JW Library is not playing media. Output is always 16:9.",
            "Choose the secondary screen JW Library uses for presentations. StageSwap watches it for media.",
            "Capture the screen JW Library shows when no media is playing. StageSwap compares the live screen with it to detect media.",
            "Check video connections, troubleshoot problems, and view logs.",
            "No media in JW Library → Zoom sees the webcam",
            "Media detected in JW Library → Zoom sees the secondary screen",
            "StageSwap sends the webcam or JW Library screen to Zoom through one virtual camera.",
            "Secondary screen",
            "Reference image",
            "Zoom output",
            "SECONDARY SCREEN",
            "REFERENCE IMAGE",
            "ZOOM OUTPUT",
            "No secondary screen frame",
            "No reference image",
            "No Zoom output frame",
            "StageSwap automatically switches what Zoom sees between the webcam and JW Library presentations. When the secondary screen matches the reference image, Zoom sees the webcam. When media is detected, Zoom sees the secondary screen. When no media is detected again, Zoom returns to the webcam.",
            "Choose the secondary screen",
            "Choose the webcam Zoom should see when JW Library is not playing media.",
            "Secondary screen preview",
            "No screen found. Connect the secondary screen used by JW Library, then rescan.",
            "This secondary screen is unavailable. Choose another one or rescan.",
            "Capture reference image",
            "Capture the screen JW Library shows when no media is playing as the reference image.",
            "Set the reference image StageSwap uses to know when to show the JW Library screen in Zoom.",
            "Show the screen JW Library shows when no media is playing, then capture the live image below.",
            "Example with no media",
            "The screen JW Library shows when no media is playing has centered text and a gray square in the corner; this example uses unbranded shapes.",
            "Prepare JW Library",
            "Show the screen JW Library shows when no media is playing. It has centered text and a gray square in the corner.",
            "Change display",
            "Choose a display",
            "What to capture",
            "Choose the secondary screen before capturing a reference image.",
            "Check the live preview, then capture the screen JW Library shows when no media is playing as the reference image.",
            "Reference image captured",
            "Capturing the current frame…",
            "No live display frame yet. Check the connection or choose another display.",
            "StageSwap will use this image to detect when JW Library is not playing media.",
            "Saved reference image",
            "Your captured image",
            "CONFIRMED",
            "TO CONFIRM",
            "No captured image available for review.",
            "Saving reference…",
            "StageSwap couldn’t save this reference. Try again or retake the image.",
            "Retake",
            "Try again",
            "Use this image",
            "Confirm reference image",
            "Make sure this image is the screen JW Library shows when no media is playing.",
            "Example reference image",
            "The screen JW Library shows when no media is playing should look like this example.",
            "Ready for the meeting",
            "Review the setup status below, then start automatic switching.",
            "Secondary screen ready",
            "Secondary screen not selected",
            "Reference image ready",
            "Reference image not captured",
            "IMPORTANT",
            "In Zoom, select Stageswap Camera as your camera before the meeting.",
            "Choose the webcam and secondary screen, then capture the screen JW Library shows when no media is playing.",
            "Live secondary screen",
            "No secondary screen image — choose a screen or use Tools in Diagnostics.",
            "Find the JW Library screen automatically",
            "Automatically fix screen capture problems",
            "Looks for the screen JW Library uses when StageSwap starts, when Settings opens, after the reference image changes, and every 30 seconds. It confirms the same screen twice before selecting it.",
            "Checks the selected screen every 30 seconds. If the image is black or missing twice in a row, StageSwap restarts screen capture.",
            "No reference image — show the screen JW Library shows when no media is playing, then capture it.",
            "Choose what StageSwap does after you sign in to Windows.",
            "Start automatic switching when StageSwap launches.",
            "Choose what happens when you close the StageSwap window.",
            "Choose whether StageSwap alerts you when something needs attention.",
            "Check whether each video component and media detection are working.",
            "Rescan for the JW Library screen or restart a video component.",
            "View the devices, formats, and timing StageSwap is currently using.",
            "Find saved settings and logs, or export logs for troubleshooting.",
            "Verbose diagnostic logging",
            "Include detailed runtime activity and periodic health checks in local logs. Enable this only while troubleshooting.",
            "StageSwap is an independent, unofficial project and is not affiliated with or endorsed by the publisher of JW Library. The name JW Library is used only to describe compatibility.",
            "Screen only",
            "The virtual camera needs attention. Restart it here, then reselect Stageswap Camera in Zoom if necessary.",
            "Still-image picture-in-picture",
            "Picture-in-picture",
            "Choose the PIP layout and optionally use it automatically for still images.",
            "Keep the speaker visible when a presentation image stays unchanged.",
            "Use picture-in-picture for still images",
            "Use automatically for still images",
            "In Auto mode, show both feeds after a non-reference image remains unchanged.",
            "Show picture-in-picture after",
            "Any screen movement restarts this timer.",
            "30 seconds",
            "45 seconds",
            "1 minute",
            "2 minutes",
            "5 minutes",
            "Main view",
            "The other live feed appears in the bottom-left inset.",
            "Used by automatic and forced PIP; the other feed appears in the bottom-left inset.",
            "Webcam full screen",
            "Secondary screen full screen",
            "PIP size",
            "Choose how much of the main view the inset covers.",
            "Mini",
            "Medium",
            "Large",
        ];

        for source in strings {
            for locale in [Locale::French, Locale::Spanish] {
                assert_ne!(
                    text(locale, source).as_ref(),
                    source,
                    "{source:?} fell back to English for {locale:?}"
                );
            }
        }
    }

    #[test]
    fn contract_catalog_does_not_restore_obsolete_user_facing_terms() {
        let source = include_str!("lib.rs");
        for obsolete in [
            ["idle", " reference"].concat(),
            ["Zoom", " retransmission"].concat(),
            ["Current pipeline", " state."].concat(),
            ["Match", " strictness"].concat(),
        ] {
            assert!(!source.contains(&obsolete), "obsolete term: {obsolete}");
        }
    }
}
