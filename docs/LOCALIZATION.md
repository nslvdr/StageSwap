# StageSwap localization guide

Use this guide when editing UI strings in `crates/i18n`. Translate tasks and visible results naturally in English, French, and neutral international Spanish. Keep labels concise; explain unfamiliar concepts nearby. See [Development](DEVELOPMENT.md#deterministic-ui-previews) to review translations in context.

## Approved terminology

| Concept | English | French | Spanish |
|---|---|---|---|
| Saved comparison screenshot | Reference image | Image de référence | Imagen de referencia |
| Monitored presentation display | Secondary screen | Écran secondaire | Pantalla secundaria |
| Final virtual-camera video | Zoom output | Sortie Zoom | Salida de Zoom |
| Main behavior | Automatic switching | Changement automatique | Cambio automático |
| Compact output modes | Auto / Camera / Screen / PIP | Auto / Caméra / Écran / Incrustation | Auto / Cámara / Pantalla / Imagen superpuesta |
| Physical camera input | Webcam | Webcam | Cámara web |
| No-content status | No media | Aucun média | Sin contenido multimedia |
| Content status | Media detected | Média détecté | Contenido multimedia detectado |
| Detection heading | Media detection | Détection des médias | Detección de contenido multimedia |
| Reference action | Capture reference image | Capturer l’image de référence | Capturar imagen de referencia |
| Comparison control | Required similarity | Similarité requise | Similitud requerida |
| Mode selector | Output mode | Mode de sortie | Modo de salida |
| Rescan and restart section | Tools | Outils | Herramientas |
| Tray recovery submenu | Recovery | Récupération | Recuperación |
| Reference-flow shortcut | Open reference capture | Ouvrir la capture de référence | Abrir captura de referencia |
| Windows privacy shortcut | Open camera privacy settings | Ouvrir les paramètres de confidentialité de la caméra | Abrir la configuración de privacidad de la cámara |
| Camera selected in Zoom | Virtual camera | Caméra virtuelle | Cámara virtual |
| Windows background area | System tray | Zone de notification | Área de notificación |
| Onboarding flow | Guided setup | Configuration guidée | Configuración guiada |
| Still-image composition | Still-image picture-in-picture | Image fixe en incrustation | Imagen fija en imagen superpuesta |

## Context rules

- Use **Secondary screen** and **Webcam** in explanations. Reserve **Screen** and **Camera** for compact output-mode controls.
- Describe the reference condition as “the screen JW Library shows when no media is playing.” Translate the idea naturally as “l’écran affiché par JW Library lorsqu’aucun média n’est en cours de lecture” and “la pantalla que muestra JW Library cuando no se reproduce contenido multimedia.”
- Use “Zoom sees” only when English copy describes the virtual-camera output. Copy about the secondary screen must describe it as the screen StageSwap watches.
- In short French states, prefer **Aucun média** and **Média détecté**. In standalone explanatory labels, **Aucun média affiché** is also approved. Do not force *contenu* into French when *média* is sufficient.
- Adapt verbs to the language instead of concatenating glossary entries. The automatic-switching actions are **Start/Stop automatic switching**, **Activer/Désactiver le changement automatique**, and **Activar/Desactivar el cambio automático**.
- Keep **StageSwap**, **JW Library**, and **Zoom** unchanged.
- Keep tray recovery labels action-oriented and consistent with the matching Diagnostics action. Opening reference capture always means entering the review-and-confirm flow; never translate it as an immediate save.
- Privacy and contention failures should name the user action. Preserve the Windows Settings destination and the examples Zoom and Teams where they appear.

## Voice and mechanics

- Use sentence case except for deliberate all-caps preview labels.
- Address French users with the formal **vous** register.
- Use direct, informal singular instructions in neutral international Spanish.
- Describe familiar tasks and outcomes instead of capture, pipeline, or recovery implementation details.
- Preserve placeholders such as `{0}` and `{1}`, meaningful line breaks, and ellipses that indicate a following dialog.
- When one English phrase appears in different contexts, give each context its own source key instead of forcing one translation to serve incompatible grammatical roles.
