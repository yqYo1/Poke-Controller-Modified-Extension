# -*- coding: utf-8 -*-
from __future__ import annotations

import os
from logging import DEBUG, Logger, NullHandler, getLogger
from typing import TYPE_CHECKING

import cv2
from numpy import argmax, array, ndarray

if TYPE_CHECKING:
    from collections.abc import Sequence
    from typing import Final, Literal

    from cv2.typing import MatLike

    type CropFmt = int | Literal["", "1", "2", "3", "4", "11", "12", "13", "14"]


def crop_image(image: MatLike, crop: list[int] | None = None) -> MatLike:
    """
    画像をトリミングする
    [y軸始点, y軸終点, x軸始点, x軸終点]
    """
    if crop is None:
        crop = []
    try:
        cropped_image = image[crop[0] : crop[1], crop[2] : crop[3]]
    except Exception:
        cropped_image = image

    return cropped_image


def crop_image_extend(
    image: MatLike,
    crop_fmt: CropFmt = "",
    crop: list[int] | None = None,
) -> MatLike:
    """
    画像をトリミングする
    ・Pillow形式
    x軸(横軸),y軸(縦軸),画像の左上が原点
    crop_fmt=1: [x軸始点, y軸始点, x軸終点, y軸終点]
    crop_fmt=2: [x軸始点, y軸始点, トリミング後の画像のサイズ(横), トリミング後の画像のサイズ(縦)]
    crop_fmt=3: [x軸始点, x軸終点, y軸始点, y軸終点]
    crop_fmt=4: [x軸始点, トリミング後の画像のサイズ(横), y軸始点, トリミング後の画像のサイズ(縦)]
    ・opencv形式(y, xの順番)
    crop_fmt=11: [y軸始点, x軸始点, y軸終点, x軸終点]
    crop_fmt=12: [y軸始点, x軸始点, トリミング後の画像のサイズ(縦), トリミング後の画像のサイズ(横)]
    crop_fmt=13: [y軸始点, y軸終点, x軸始点, x軸終点]
    crop_fmt=14: [y軸始点, トリミング後の画像のサイズ(縦), x軸始点, トリミング後の画像のサイズ(横)]
    """
    if crop is None:
        crop = []

    try:
        # pillow形式
        if crop_fmt in {1, "1"}:
            cropped_image = image[crop[1] : crop[3], crop[0] : crop[2]]
        elif crop_fmt in {2, "2"}:
            cropped_image = image[
                crop[1] : crop[1] + crop[3],
                crop[0] : crop[0] + crop[2],
            ]
        elif crop_fmt in {3, "3"}:
            cropped_image = image[crop[2] : crop[3], crop[0] : crop[1]]
        elif crop_fmt in {4, "4"}:
            cropped_image = image[
                crop[2] : crop[2] + crop[3],
                crop[0] : crop[0] + crop[1],
            ]
        # opencv形式
        elif crop_fmt in {11, "11"}:
            cropped_image = image[crop[0] : crop[2], crop[1] : crop[3]]
        elif crop_fmt in {12, "12"}:
            cropped_image = image[
                crop[0] : crop[0] + crop[2],
                crop[1] : crop[1] + crop[3],
            ]
        elif crop_fmt in {13, "13"}:
            cropped_image = image[crop[0] : crop[1], crop[2] : crop[3]]
        elif crop_fmt in {14, "14"}:
            cropped_image = image[
                crop[0] : crop[0] + crop[1],
                crop[2] : crop[2] + crop[3],
            ]
        else:
            cropped_image = image
    except Exception:
        cropped_image = image

    return cropped_image


def getInterframeDiff(
    frame1: MatLike,
    frame2: MatLike,
    frame3: MatLike,
    threshold: float,
) -> MatLike:
    """
    Get interframe difference binarized image
    フレーム間差分により2値化された画像を取得する
    """
    diff1 = cv2.absdiff(frame1, frame2)
    diff2 = cv2.absdiff(frame2, frame3)

    diff = cv2.bitwise_and(diff1, diff2)

    # binarize
    img_th = cv2.threshold(diff, threshold, 255, cv2.THRESH_BINARY)[1]

    # remove noise
    return cv2.medianBlur(img_th, 3)


def getImage(
    path: str,
    mode: Literal["color", "binary", "gray"] = "color",
) -> MatLike | None:
    """
    画像の読み込みを行う。
    """
    if path:
        try:
            if mode == "color":
                return cv2.imread(path, cv2.IMREAD_COLOR)
            if mode == "gray":
                return cv2.imread(path, cv2.IMREAD_GRAYSCALE)
            if mode == "binary":
                return cv2.imread(path, 0)
            print(f'mode={mode}が不正です。mode="color"として読み込みます。')  # pyright:ignore[reportUnreachable]
            return cv2.imread(path, cv2.IMREAD_COLOR)
        except Exception:
            print(
                f"{path}が開けませんでした。ファイル名およびファイルの格納場所を確認してください。",
            )
            return None
    else:
        return None


def doPreprocessImage(
    image: MatLike,
    use_gray: bool = True,
    crop: list[int] | None = None,
    BGR_range: dict[Literal["lower", "upper"], int | tuple[int, int, int]]
    | None = None,
    threshold_binary: int | None = None,
) -> tuple[MatLike, int, int]:
    """
    画像をトリミングしてグレースケール化/2値化する
    2値化関連のContributor: mikan kochan 空太 (敬称略)
    """
    src = crop_image(image, crop=crop)  # トリミング

    if use_gray:
        src = cv2.cvtColor(src, cv2.COLOR_BGR2GRAY)  # グレースケール化
    elif BGR_range is not None:  # 2値化
        src = cv2.inRange(
            src,
            array(BGR_range["lower"]),
            array(BGR_range["upper"]),
        )  # inRangeで元画像を2値化(指定した色の範囲を抽出できる)

    if threshold_binary is not None:
        _, src = cv2.threshold(src, threshold_binary, 255, cv2.THRESH_BINARY)

    # テンプレート画像のサイズ
    width, height = src.shape[1], src.shape[0]

    return src, width, height


def opneImage(
    image: MatLike,
    crop: list[int] | None = None,
    title: str = "image",
) -> None:
    """
    キー入力があるまで画像を表示する
    Contributor: kochan (敬称略)
    """
    src = crop_image(image, crop=crop)  # トリミング
    cv2.imshow(f"{title}", src)
    cv2.waitKey(0)
    cv2.destroyAllWindows()


class ImageProcessing:
    """
    画像に関する処理を行う。
    """

    __logger: Logger
    __activate_logger = False
    __gsrc = None
    __gtmpl = None
    __gresult = None
    __use_gpu = False
    image_type: Final = ndarray

    def __init__(self, use_gpu: bool = False) -> None:
        # ロガーを起動する(1回だけ)
        if not self.__activate_logger:
            self.__logger = getLogger(__name__)
            self.__logger.addHandler(NullHandler())
            self.__logger.setLevel(DEBUG)
            self.__logger.propagate = True
        # GUP使用時、デバイスのメモリを確保する(1回だけ)
        if use_gpu:
            """
            テンプレートマッチングをGPUで行うようにする。
            うまく設定できたらuse_gpuのフラグをTrueにする。
            """
            try:
                if self.__gsrc is None:
                    self.__gsrc = cv2.cuda_GpuMat()  # pyright: ignore[reportAttributeAccessIssue]
                if self.__gtmpl is None:
                    self.__gtmpl = cv2.cuda_GpuMat()  # pyright: ignore[reportAttributeAccessIssue]
                if self.__gresult is None:
                    self.__gresult = cv2.cuda_GpuMat()  # pyright: ignore[reportAttributeAccessIssue]
                self.__use_gpu = True
                print("template matching:mask is ignored.")
            except Exception:
                self.__use_gpu = False
        else:
            self.__use_gpu = False

    def imwrite(
        self,
        filename: str,
        image: MatLike,
        params: Sequence[int] | None = None,
    ) -> bool:
        """
        画像を書き込む
        """
        if params is None:
            params = []
        try:
            ext = os.path.splitext(filename)[1]
            result, n = cv2.imencode(ext, image, params)

            if result:
                with open(filename, mode="w+b") as f:
                    n.tofile(f)
                return True
            return False
        except Exception as e:
            print(e)
            self.__logger.error(f"Image Write Error: {e}")
            return False

    def doTemplateMatch(
        self,
        image: MatLike,
        template_image: MatLike,
        mask_image: MatLike | None = None,
    ) -> tuple[float, Sequence[int]]:
        """
        テンプレートマッチングをする
        画像は必要に応じて事前にグレースケール化やトリミングをしておく必要がある
        """
        # 比較方式を設定する
        method = (
            cv2.TM_CCORR_NORMED
            if isinstance(mask_image, ndarray)
            else cv2.TM_CCOEFF_NORMED
        )

        # テンプレートマッチングをする
        if self.__use_gpu:  # GPUを使用する場合(マスク非対応)
            print("template matching mode:GPU")
            self.__gsrc.upload(image)  # pyright: ignore[reportOptionalMemberAccess]
            self.__gtmpl.upload(template_image)  # pyright: ignore[reportOptionalMemberAccess]
            matcher = cv2.cuda.createTemplateMatching(cv2.CV_8UC1, method)  # pyright: ignore[reportAttributeAccessIssue,reportUnknownVariableType]
            self.__gresult = matcher.match(self.__gsrc, self.__gtmpl)
            res = self.__gresult.download()  # pyright: ignore[reportUnknownVariableType]
        else:
            res = cv2.matchTemplate(image, template_image, method, mask=mask_image)
        _, max_val, _, max_loc = cv2.minMaxLoc(
            res,  # pyright: ignore[reportUnknownArgumentType]
        )  # 結果から類似度と類似度が最大となる場所を抽出

        return max_val, max_loc

    def isContainTemplate(
        self,
        image: MatLike,
        template_image: MatLike,
        mask_image: MatLike | None = None,
        threshold: float = 0.7,
        use_gray: bool = True,
        crop: list[int] | None = None,
        BGR_range: dict[Literal["lower", "upper"], int | tuple[int, int, int]]
        | None = None,
        threshold_binary: int | None = None,
        crop_template: list[int] | None = None,
        show_image: bool = False,
    ) -> tuple[bool, Sequence[int], int, int, float]:
        """
        テンプレートマッチングを行い類似度が閾値を超えているかを確認する
        """
        # テンプレートマッチング対象画像を加工する
        src, _, _ = doPreprocessImage(
            image,
            use_gray=use_gray,
            crop=crop,
            BGR_range=BGR_range,
            threshold_binary=threshold_binary,
        )

        # [DEBUG] テンプレートマッチング対象画像を表示する
        if show_image:
            cv2.imshow("image", src)
            cv2.waitKey()

        # テンプレート画像を加工する
        template, width, height = doPreprocessImage(
            template_image,
            use_gray=use_gray,
            crop=crop_template,
            BGR_range=BGR_range,
            threshold_binary=threshold_binary,
        )

        # テンプレートマッチングを行う
        max_val, max_loc = self.doTemplateMatch(src, template, mask_image=mask_image)

        # 類似度が閾値を超えたかを戻り値として返す(合わせて位置とテンプレート画像のサイズも返す)
        return max_val > threshold, max_loc, width, height, max_val

    def isContainTemplate_max(
        self,
        image: MatLike,
        template_image_list: list[MatLike],
        mask_image_list: list[MatLike | None] | None = None,
        threshold: float = 0.7,
        use_gray: bool = True,
        crop: list[int] | None = None,
        BGR_range: dict[Literal["lower", "upper"], int | tuple[int, int, int]]
        | None = None,
        threshold_binary: int | None = None,
        crop_template: list[int] | None = None,
        show_image: bool = False,
    ) -> tuple[int, list[float], list[Sequence[int]], list[int], list[int], list[bool]]:
        """
        複数のテンプレート画像を用いてそれぞれテンプレートマッチングを行い類似度が最も大きい画像のindexを返す
        """
        # パラメータチェックを行う
        if mask_image_list is None or len(mask_image_list) == 0:
            mask_image_list_temp = [None for i in range(len(template_image_list))]
        elif len(template_image_list) == len(mask_image_list):
            mask_image_list_temp = mask_image_list
        else:
            print("The number of template images and mask images don't match. ")
            return -1, [], [], [], [], []

        # ループをまわしてテンプレート画像数分テンプレートマッチングを行う
        max_val_list: list[float] = []
        max_loc_list: list[Sequence[int]] = []
        width_list: list[int] = []
        height_list: list[int] = []
        judge_threshold_list: list[bool] = []

        # テンプレートマッチング対象画像を加工する
        src, _, _ = doPreprocessImage(
            image,
            use_gray=use_gray,
            crop=crop,
            BGR_range=BGR_range,
            threshold_binary=threshold_binary,
        )

        # [DEBUG] テンプレートマッチング対象画像を表示する
        if show_image:
            cv2.imshow("image", src)
            cv2.waitKey()

        for template_image, mask_image in zip(
            template_image_list,
            mask_image_list_temp,
            strict=False,
        ):
            # テンプレート画像を加工する
            template, width, height = doPreprocessImage(
                template_image,
                use_gray=use_gray,
                crop=crop_template,
                BGR_range=BGR_range,
                threshold_binary=threshold_binary,
            )
            max_val, max_loc = self.doTemplateMatch(
                src,
                template,
                mask_image=mask_image,
            )
            max_val_list.append(max_val)
            max_loc_list.append(max_loc)
            width_list.append(width)
            height_list.append(height)
            judge_threshold_list.append(max_val > threshold)

        return (
            int(argmax(max_val_list)),
            max_val_list,
            max_loc_list,
            width_list,
            height_list,
            judge_threshold_list,
        )

    def saveImage(
        self,
        image: MatLike,
        filename: str,
        crop: list[int] | None = None,
    ) -> None:
        """
        画像を保存する。
        """
        # トリミングを行う
        cropped_image = crop_image(image, crop=crop)

        # ファイル名からパスを抽出する
        capture_dir = os.path.dirname(filename)

        # 画像保存用ディレクトリの存在を確認し、なかったら作成する。
        if not os.path.exists(capture_dir):
            os.makedirs(capture_dir)
            self.__logger.debug("Created Capture folder")

        # 画像を保存する
        try:
            self.imwrite(filename, cropped_image)
            self.__logger.debug(f"Capture succeeded: {filename}")
            print("capture succeeded: " + filename)
        except cv2.error as e:
            print("Capture Failed")
            self.__logger.error(f"Capture Failed :{e}")
